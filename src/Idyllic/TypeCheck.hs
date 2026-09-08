-- | Bidirectional type checking / elaboration for the CoC core.
--
--   The checker is the standard two-mode pair:
--
--     * 'infer' synthesises a type from a term (@e => A@);
--     * 'check' verifies a term against a known type (@e \<= A@).
--
--   Everything self-describing lives in 'infer'; everything that wants to be
--   told its type (a lambda) is handled specially in 'check'; and the two
--   meet in 'check''s fallback, which infers and then unifies.  Both modes
--   elaborate to 'Core' as they go, because building a 'VPi' needs a
--   first-order closure over a term, not just a value.
--
--   __Metavariables.__  Wherever the elaborator needs a type it has not been
--   given — the domain of an unannotated lambda, an explicit @_@ hole — it
--   invents a fresh metavariable and lets
--   'Idyllic.TypeCheck.Unify.unify' work out what it must have been.  Metas
--   are created /closed/: a meta made in context @Γ@ is immediately applied
--   to the bound variables of @Γ@ (see 'Idyllic.TypeCheck.Monad.boundVarCores'),
--   so a solution is a closed term that can be substituted anywhere.  Any
--   meta still unsolved at the end is reported.
--
--   Every node's type is written to a side table keyed by its 'H.NodeId'
--   (see "Idyllic.TypeCheck.Monad"), so a later pass — or an editor — can ask
--   for the type at a source position by pairing that table with the
--   renamer's 'H.SpanTable'.
--
--   Two deliberate limitations for now:
--
--     * The sort rule is @Type : Type@, which is convenient and logically
--       inconsistent.  Universe levels are the obvious next step.
--
--     * Members of a recursive 'H.Rec' group must carry a signature, and are
--       bound /opaquely/ so they never unfold during unification.  That is
--       what keeps unification terminating, and it is exactly why the SCC
--       pass runs first: only genuinely recursive definitions pay the price,
--       while 'H.NonRec' definitions stay transparent.
module Idyllic.TypeCheck
  ( TypeCheckResult (..),
    checkModule,
    infer,
    check,

    -- * Re-exports
    TypeError (..),
    TypeErrorKind (..),
    TypeTable,
    MetaCxt,
    MetaEntry (..),
    typeErrorReport,
    Core (..),
    showCore,
  )
where

import Control.Monad (foldM, forM, forM_, unless)
import Data.IntMap.Strict qualified as IntMap
import Data.Text (Text)
import Idyllic.Syntax.HIR qualified as H
import Idyllic.TypeCheck.Core
import Idyllic.TypeCheck.Eval
import Idyllic.TypeCheck.Monad
import Idyllic.TypeCheck.Unify
import Idyllic.TypeCheck.Value

-- ---------------------------------------------------------------------------
-- Entry point
-- ---------------------------------------------------------------------------

data TypeCheckResult = TypeCheckResult
  { -- | The type of the module's trailing expression.
    tcrBodyType :: Core,
    -- | Node id to type, for every node the checker visited.
    tcrNodeTypes :: TypeTable,
    -- | The final metacontext, so callers can inspect what was solved.
    tcrMetas :: MetaCxt,
    tcrErrors :: [TypeError]
  }
  deriving (Show)

-- | Type check a renamed, SCC-ordered module.  Never throws: every problem
--   becomes a 'TypeError' and checking continues with 'VError' standing in
--   for the offending subterm.
checkModule :: H.Module -> TypeCheckResult
checkModule m = TypeCheckResult ty tys metas errs
  where
    (ty, tys, metas, errs) = runTC $ do
      ctx <- foldM checkGroup initialCtx (H.modGroups m)
      (_, bodyTy) <- infer ctx (H.modBody m)
      result <- quoteAt ctx bodyTy
      reportHoles
      reportUnsolvedMetas
      pure result

-- | Anything unification never pinned down is a genuine gap in the program.
--   Run once, at the very end, so a meta solved late still counts as solved.
reportUnsolvedMetas :: TC s ()
reportUnsolvedMetas = do
  metas <- getMetaCxt
  forM_ (IntMap.elems metas) $ \case
    -- holes report themselves, below, so they are not counted twice
    Unsolved (MetaInfo _ MetaHole) -> pure ()
    Unsolved (MetaInfo nid (MetaInferred desc)) -> typeError nid (UnsolvedMeta desc)
    Solved _ _ -> pure ()

-- | Report every @_@ in the source.  Run at the very end, so a hole that
--   unification filled in reports the term it was filled with rather than
--   just its type.
reportHoles :: TC s ()
reportHoles = do
  holes <- getHoles
  forM_ holes $ \h -> do
    ty <- quote (hoLvl h) (hoType h)
    v <- force (hoValue h)
    case v of
      VFlex _ _ -> typeError (hoNode h) (HoleUnsolved ty)
      _ -> do
        solution <- quote (hoLvl h) v
        typeError (hoNode h) (HoleSolved ty solution)

-- | Create a fresh metavariable, already applied to the bound variables of
--   the context, and return the Core term standing for it.
freshMetaWith :: Ctx -> H.NodeId -> MetaOrigin -> TC s Core
freshMetaWith ctx nid origin = do
  m <- newMeta (MetaInfo nid origin)
  pure (foldl CApp (CMeta m) (boundVarCores ctx))

freshMeta :: Ctx -> H.NodeId -> Text -> TC s Core
freshMeta ctx nid desc = freshMetaWith ctx nid (MetaInferred desc)

-- | 'freshMeta', already evaluated.
freshMetaValue :: Ctx -> H.NodeId -> Text -> TC s (Core, Value)
freshMetaValue ctx nid desc = do
  c <- freshMeta ctx nid desc
  v <- eval (ctxEnv ctx) c
  pure (c, v)

-- | Elaborate an explicit @_@: a meta for its value, remembered so it can be
--   reported once unification has had a chance to fill it in.
elabHole :: Ctx -> H.NodeId -> VTy -> TC s Core
elabHole ctx nid ty = do
  c <- freshMetaWith ctx nid MetaHole
  v <- eval (ctxEnv ctx) c
  recordHole (HoleInfo (ctxLvl ctx) nid v ty)
  pure c

-- ---------------------------------------------------------------------------
-- Bindings
-- ---------------------------------------------------------------------------

-- | Check one dependency-ordered group, returning the context extended with
--   its bindings.
checkGroup :: Ctx -> H.BindGroup -> TC s Ctx
checkGroup ctx (H.NonRec b) = do
  (rhs, ty) <- case H.bindType b of
    -- no signature: infer, and let the definition be transparent
    Nothing -> infer ctx (H.bindRhs b)
    Just sig -> do
      sigC <- check ctx sig VType
      vsig <- eval (ctxEnv ctx) sigC
      rhs <- check ctx (H.bindRhs b) vsig
      pure (rhs, vsig)
  recordNodeType (H.bindId b) =<< quoteAt ctx ty
  v <- eval (ctxEnv ctx) rhs
  pure (defineVar ctx (H.bindName b) ty v)
checkGroup ctx (H.Rec bs) = do
  -- 1. every member of a recursive group needs a signature; without one there
  --    is nothing to check the recursive occurrences against.
  sigs <- forM bs $ \b -> case H.bindType b of
    Nothing -> do
      typeError (H.bindId b) (RecursiveBindingNeedsSignature (H.idText (H.bindName b)))
      pure VError
    Just sig -> do
      sigC <- check ctx sig VType
      eval (ctxEnv ctx) sigC
  -- 2. assume all of them, opaquely, so unification cannot unfold a
  --    recursive definition forever.
  let assume c (b, ty) = bindVar c (Just (H.bindName b)) (H.idText (H.bindName b)) ty
      ctxRec = foldl assume ctx (zip bs sigs)
  -- 3. check each right-hand side against its own signature.
  forM_ (zip bs sigs) $ \(b, ty) -> do
    _ <- check ctxRec (H.bindRhs b) ty
    recordNodeType (H.bindId b) =<< quoteAt ctxRec ty
  pure ctxRec

-- ---------------------------------------------------------------------------
-- Inference
-- ---------------------------------------------------------------------------

-- | Synthesise the type of a term, elaborating it to 'Core' on the way.
infer :: Ctx -> H.Term -> TC s (Core, VTy)
infer ctx (H.Term nid kind) = do
  result@(_, ty) <- case kind of
    H.HType -> pure (CType, VType)
    H.HLit l -> pure (CLit l, VLitTy (litBuiltin l))
    -- a hole in inference position needs a meta for its type as well as for
    -- its value
    H.HHole -> do
      (_, tyV) <- freshMetaValue ctx nid "the type of this hole"
      tmC <- elabHole ctx nid tyV
      pure (tmC, tyV)
    H.HVar x
      -- the renamer already reported this one; stay quiet and keep going
      | H.idUnique x < 0 -> pure (CError, VError)
      | otherwise -> case lookupVar ctx x of
          Just r -> pure r
          Nothing -> do
            typeError nid (UnknownVariable (H.idText x))
            pure (CError, VError)
    H.HAnn e sig -> do
      sigC <- check ctx sig VType
      vsig <- eval (ctxEnv ctx) sigC
      eC <- check ctx e vsig
      pure (eC, vsig)
    H.HApp f a -> do
      (fC, fTy) <- infer ctx f
      fTy' <- force fTy
      case fTy' of
        VPi _ dom cod -> do
          aC <- check ctx a dom
          aV <- eval (ctxEnv ctx) aC
          resTy <- cod $$ aV
          pure (CApp fC aC, resTy)
        VError -> pure (CError, VError)
        -- the head's type is not known to be a function yet: if it is still
        -- flexible, force it to be one by unifying with a fresh Pi
        VFlex _ _ -> do
          (_, domV) <- freshMetaValue ctx (H.termId f) "the argument type of this function"
          let ctxArg = bindVar ctx Nothing "x" domV
          codC <- freshMeta ctxArg (H.termId f) "the result type of this function"
          let piV = VPi "x" domV (Closure (ctxEnv ctx) codC)
          ok <- unify (ctxLvl ctx) fTy' piV
          if not ok
            then do
              q <- quoteAt ctx fTy'
              typeError (H.termId f) (NotAFunction q)
              pure (CError, VError)
            else do
              aC <- check ctx a domV
              aV <- eval (ctxEnv ctx) aC
              resTy <- Closure (ctxEnv ctx) codC $$ aV
              pure (CApp fC aC, resTy)
        other -> do
          q <- quoteAt ctx other
          typeError (H.termId f) (NotAFunction q)
          pure (CError, VError)
    H.HPi mx dom cod -> do
      domC <- check ctx dom VType
      domV <- eval (ctxEnv ctx) domC
      let hint = maybe "_" H.idText mx
          ctx' = bindVar ctx mx hint domV
      codC <- check ctx' cod VType
      pure (CPi hint domC codC, VType)
    H.HLam x mann body -> do
      let hint = H.idText x
      -- with metavariables an unannotated binder is no longer a dead end:
      -- invent its type and let unification discover it
      annV <- case mann of
        Just ann -> do
          annC <- check ctx ann VType
          eval (ctxEnv ctx) annC
        Nothing -> snd <$> freshMetaValue ctx nid ("the type of the binder " <> hint)
      let ctx' = bindVar ctx (Just x) hint annV
      (bodyC, bodyTy) <- infer ctx' body
      -- the codomain may mention the binder, so quote it one level in and
      -- close it over the *outer* environment
      codC <- quoteAt ctx' bodyTy
      pure (CLam hint bodyC, VPi hint annV (Closure (ctxEnv ctx) codC))
    H.HLet groups body -> do
      ctx' <- foldM checkGroup ctx groups
      -- the let-bound names became context entries, so the body's Core and
      -- type are already meaningful in ctx' — which is where they are used
      infer ctx' body
  recordNodeType nid =<< quoteAt ctx ty
  pure result

-- ---------------------------------------------------------------------------
-- Checking
-- ---------------------------------------------------------------------------

-- | Check a term against an expected type, elaborating it to 'Core'.
check :: Ctx -> H.Term -> VTy -> TC s Core
check ctx tm@(H.Term nid kind) expected0 = do
  expected <- force expected0
  core <- case (kind, expected) of
    -- a lambda gets its binder type from the expected Pi
    (H.HLam x mann body, VPi _ dom cod) -> do
      forM_ mann $ \ann -> do
        annC <- check ctx ann VType
        annV <- eval (ctxEnv ctx) annC
        ok <- unify (ctxLvl ctx) annV dom
        unless ok $ do
          d <- quoteAt ctx dom
          a <- quoteAt ctx annV
          typeError (H.termId ann) (BinderMismatch d a)
      let hint = H.idText x
          ctx' = bindVar ctx (Just x) hint dom
      codV <- cod $$ vVar (ctxLvl ctx) hint
      bodyC <- check ctx' body codV
      pure (CLam hint bodyC)
    -- a hole is exactly what a metavariable is for: its type is whatever was
    -- expected here, so nothing needs to be inferred
    (H.HHole, _) -> elabHole ctx nid expected
    (H.HLet groups body, _) -> do
      ctx' <- foldM checkGroup ctx groups
      check ctx' body expected
    -- everything else: infer, then unify.  An expected type that is still a
    -- flexible meta gets solved here rather than needing a special case.
    _ -> do
      (core, actual) <- infer ctx tm
      ok <- unify (ctxLvl ctx) actual expected
      unless ok $ do
        e <- quoteAt ctx expected
        a <- quoteAt ctx actual
        typeError nid (TypeMismatch e a)
      pure core
  -- record the *expected* type; it is the more informative one, and it
  -- overwrites whatever 'infer' may have just written for this node
  recordNodeType nid =<< quoteAt ctx expected
  pure core
