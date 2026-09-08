-- | The type checker's effects, its metacontext, and its typing context.
--
--   Three things want to be mutable here, and all three are genuinely
--   mutable rather than merely threaded:
--
--     * the /metacontext/ — solving a metavariable has to be visible to every
--       value that already mentions it, everywhere, immediately;
--
--     * the /type side table/, written once per HIR node visited;
--
--     * the /error log/.
--
--   So the pass runs in 'ST' over a handful of 'STRef's and the recursion
--   stays a plain reader.  'runTC' keeps that private: the checker is pure
--   from the outside.
module Idyllic.TypeCheck.Monad
  ( -- * The monad
    TC,
    TcRefs (..),
    runTC,
    liftST,

    -- * Metavariables
    MetaVar,
    MetaEntry (..),
    MetaInfo (..),
    MetaOrigin (..),
    MetaCxt,
    newMeta,
    lookupMeta,
    solveMeta,
    getMetaCxt,

    -- * Holes
    HoleInfo (..),
    recordHole,
    getHoles,

    -- * Side tables
    TypeTable,
    recordNodeType,

    -- * Errors
    TypeError (..),
    TypeErrorKind (..),
    typeError,
    typeErrorReport,

    -- * Contexts
    Ctx (..),
    BinderInfo (..),
    initialCtx,
    bindVar,
    defineVar,
    lookupVar,
    boundVarCores,
  )
where

import Control.Monad.Reader (ReaderT, asks, lift, runReaderT)
import Control.Monad.ST (ST, runST)
import Data.IntMap.Strict (IntMap)
import Data.IntMap.Strict qualified as IntMap
import Data.STRef
import Data.Text (Text)
import Error.Diagnose (Marker (..), Position (..), Report, err, warn)
import Idyllic.Builtins
import Idyllic.Syntax.HIR qualified as H
import Idyllic.TypeCheck.Core
import Idyllic.TypeCheck.Value
import Idyllic.Utils (LineIndex, Span (..), offsetToLineCol)

-- ---------------------------------------------------------------------------
-- Monad
-- ---------------------------------------------------------------------------

data TcRefs s = TcRefs
  { -- | Node id (as 'Int') to that node's type, quoted in the node's context.
    tcNodeTypes :: STRef s TypeTable,
    -- | Errors, newest first.
    tcErrors :: STRef s [TypeError],
    -- | The metacontext.
    tcMetas :: STRef s MetaCxt,
    -- | Next metavariable to hand out.
    tcNextMeta :: STRef s MetaVar,
    -- | Explicit @_@ holes, newest first, reported once checking is over.
    tcHoles :: STRef s [HoleInfo]
  }

type TC s = ReaderT (TcRefs s) (ST s)

liftST :: ST s a -> TC s a
liftST = lift

-- | Run a checking action, returning its result alongside the side table, the
--   final metacontext and the errors it accumulated.  The 'ST' state never
--   escapes.
runTC :: (forall s. TC s a) -> (a, TypeTable, MetaCxt, [TypeError])
runTC action = runST $ do
  tyRef <- newSTRef IntMap.empty
  errRef <- newSTRef []
  metaRef <- newSTRef IntMap.empty
  nextRef <- newSTRef 0
  holeRef <- newSTRef []
  a <- runReaderT action (TcRefs tyRef errRef metaRef nextRef holeRef)
  tys <- readSTRef tyRef
  errs <- readSTRef errRef
  metas <- readSTRef metaRef
  pure (a, tys, metas, reverse errs)

-- ---------------------------------------------------------------------------
-- Metavariables
-- ---------------------------------------------------------------------------

-- | Why a metavariable was created.
data MetaOrigin
  = -- | Stands for an explicit @_@; reported by the hole pass instead of the
    --   generic unsolved-meta pass, so it is never reported twice.
    MetaHole
  | -- | Invented by the elaborator, with a description of what it stands for.
    MetaInferred Text
  deriving (Show)

-- | Where a metavariable came from, so an unsolved one can be reported at a
--   sensible place with a sensible message.
data MetaInfo = MetaInfo
  { miNode :: H.NodeId,
    miOrigin :: MetaOrigin
  }
  deriving (Show)

-- | A hole, remembered so it can be reported /after/ checking, by which time
--   unification may have worked out what belongs in it.
data HoleInfo = HoleInfo
  { -- | the hole's context depth, for quoting
    hoLvl :: Lvl,
    hoNode :: H.NodeId,
    -- | the meta standing for the hole's value, applied to its spine
    hoValue :: Value,
    hoType :: VTy
  }

data MetaEntry
  = Unsolved MetaInfo
  | Solved Value MetaInfo
  deriving (Show)

type MetaCxt = IntMap MetaEntry

newMeta :: MetaInfo -> TC s MetaVar
newMeta info = do
  nextRef <- asks tcNextMeta
  metaRef <- asks tcMetas
  liftST $ do
    m <- readSTRef nextRef
    writeSTRef nextRef (m + 1)
    modifySTRef' metaRef (IntMap.insert m (Unsolved info))
    pure m

lookupMeta :: MetaVar -> TC s MetaEntry
lookupMeta m = do
  metaRef <- asks tcMetas
  metas <- liftST (readSTRef metaRef)
  case IntMap.lookup m metas of
    Just e -> pure e
    -- cannot happen: metas are only produced by 'newMeta'
    Nothing -> pure (Unsolved (MetaInfo (H.NodeId (-1)) (MetaInferred "unknown")))

solveMeta :: MetaVar -> Value -> TC s ()
solveMeta m v = do
  metaRef <- asks tcMetas
  liftST $ modifySTRef' metaRef (IntMap.adjust solve m)
  where
    solve (Unsolved info) = Solved v info
    solve e@(Solved _ _) = e

getMetaCxt :: TC s MetaCxt
getMetaCxt = liftST . readSTRef =<< asks tcMetas

recordHole :: HoleInfo -> TC s ()
recordHole h = do
  ref <- asks tcHoles
  liftST $ modifySTRef' ref (h :)

-- | Holes in source order.
getHoles :: TC s [HoleInfo]
getHoles = reverse <$> (liftST . readSTRef =<< asks tcHoles)

-- ---------------------------------------------------------------------------
-- Side tables
-- ---------------------------------------------------------------------------

-- | Side table keyed by 'H.NodeId' (as its underlying 'Int'), mirroring
--   'H.SpanTable'.  Together they let any later pass recover both the source
--   location and the type of an HIR node without either being stored inline.
type TypeTable = IntMap Core

recordNodeType :: H.NodeId -> Core -> TC s ()
recordNodeType (H.NodeId n) ty = do
  ref <- asks tcNodeTypes
  liftST $ modifySTRef' ref (IntMap.insert n ty)

-- ---------------------------------------------------------------------------
-- Errors
-- ---------------------------------------------------------------------------

-- | A message attributed to the HIR node it was found at; the span comes from
--   the renamer's 'H.SpanTable'.
data TypeError = TypeError
  { teNode :: H.NodeId,
    teKind :: TypeErrorKind
  }
  deriving (Show)

data TypeErrorKind
  = -- | expected, actual
    TypeMismatch Core Core
  | -- | the type of the thing being applied
    NotAFunction Core
  | -- | binder annotation disagrees with the expected domain: expected, given
    BinderMismatch Core Core
  | -- | a member of a recursive group with no signature
    RecursiveBindingNeedsSignature Text
  | -- | should not happen if the renamer ran; defensive
    UnknownVariable Text
  | -- | a metavariable that unification never pinned down
    UnsolvedMeta Text
  | -- | an explicit @_@ that unification filled in: type, then solution
    HoleSolved Core Core
  | -- | an explicit @_@ that nothing determined: its type
    HoleUnsolved Core
  deriving (Show)

typeError :: H.NodeId -> TypeErrorKind -> TC s ()
typeError nid kind = do
  ref <- asks tcErrors
  liftST $ modifySTRef' ref (TypeError nid kind :)

typeErrorReport :: FilePath -> LineIndex -> H.SpanTable -> TypeError -> Report Text
typeErrorReport file li spans (TypeError nid kind) = case kind of
  TypeMismatch expected actual ->
    err
      Nothing
      "type mismatch"
      [(here, This ("expected " <> showCore expected <> ", but this has type " <> showCore actual))]
      []
  NotAFunction ty ->
    err
      Nothing
      "this is not a function"
      [(here, This ("it has type " <> showCore ty <> ", so it cannot be applied"))]
      []
  BinderMismatch expected given ->
    err
      Nothing
      "binder annotation does not match the expected argument type"
      [(here, This ("expected " <> showCore expected <> ", but the binder says " <> showCore given))]
      []
  RecursiveBindingNeedsSignature name ->
    err
      Nothing
      ("the recursive binding " <> name <> " needs a type signature")
      [(here, This "its type cannot be inferred from a recursive definition")]
      ["give it a signature, as in " <> name <> " : .. = .."]
  UnknownVariable name ->
    err Nothing ("not in scope: " <> name) [(here, This "unbound here")] []
  UnsolvedMeta desc ->
    err
      Nothing
      "unsolved metavariable"
      [(here, This ("could not infer " <> desc))]
      ["add a type annotation to pin it down"]
  HoleSolved ty solution ->
    warn
      Nothing
      "found hole"
      [(here, This ("_ : " <> showCore ty <> "  =  " <> showCore solution))]
      []
  HoleUnsolved ty ->
    err
      Nothing
      "unsolved hole"
      [(here, This ("_ : " <> showCore ty <> ", and nothing determines it"))]
      []
  where
    here = case IntMap.lookup (H.unNodeId nid) spans of
      Just (Span s e) ->
        let (l1, c1) = offsetToLineCol li s
            (l2, c2) = offsetToLineCol li e
         in Position (l1, c1) (l2, c2) file
      Nothing -> Position (1, 1) (1, 1) file

-- ---------------------------------------------------------------------------
-- Contexts
-- ---------------------------------------------------------------------------

-- | Whether a context entry is an opaque binder or a transparent definition.
--   Only 'Bound' entries go into a fresh meta's spine: a meta must depend on
--   the variables it could legitimately mention, and a definition is not one
--   of those (it already has a value).
data BinderInfo = Bound | Defined
  deriving (Show, Eq)

-- | The typing context.
--
--   'ctxEnv', 'ctxBinders' and 'ctxNames' are parallel and innermost-first.
--   'ctxEnv' holds a neutral variable for an opaque binding (a lambda\/Pi
--   binder, or a member of a recursive group) and the binding's value for a
--   transparent definition, which then unfolds during unification.
data Ctx = Ctx
  { ctxLvl :: Lvl,
    ctxEnv :: Env,
    ctxBinders :: [BinderInfo],
    ctxNames :: [Text],
    -- | name unique to (the level it was bound at, its type)
    ctxTypes :: IntMap (Lvl, VTy)
  }

emptyCtx :: Ctx
emptyCtx = Ctx 0 [] [] [] IntMap.empty

-- | The context every module starts in: the primitive type constants, each a
--   transparent definition of sort @Type@.  Their levels line up with
--   'builtinUnique', matching the renamer's seeded global scope.
initialCtx :: Ctx
initialCtx = foldl add emptyCtx allBuiltins
  where
    add ctx b =
      Ctx
        { ctxLvl = ctxLvl ctx + 1,
          ctxEnv = VLitTy b : ctxEnv ctx,
          ctxBinders = Defined : ctxBinders ctx,
          ctxNames = builtinName b : ctxNames ctx,
          ctxTypes = IntMap.insert (builtinUnique b) (ctxLvl ctx, VType) (ctxTypes ctx)
        }

-- | Extend with an opaque binder.  The 'Maybe' is for the anonymous binder of
--   a non-dependent arrow: it still occupies a slot so that indices line up,
--   it just is not nameable.
bindVar :: Ctx -> Maybe H.Ident -> Text -> VTy -> Ctx
bindVar ctx mx hint ty =
  Ctx
    { ctxLvl = lvl + 1,
      ctxEnv = vVar lvl hint : ctxEnv ctx,
      ctxBinders = Bound : ctxBinders ctx,
      ctxNames = hint : ctxNames ctx,
      ctxTypes = case mx of
        Nothing -> ctxTypes ctx
        Just x -> IntMap.insert (H.idUnique x) (lvl, ty) (ctxTypes ctx)
    }
  where
    lvl = ctxLvl ctx

-- | Extend with a transparent definition, which unfolds during unification.
defineVar :: Ctx -> H.Ident -> VTy -> Value -> Ctx
defineVar ctx x ty v =
  Ctx
    { ctxLvl = ctxLvl ctx + 1,
      ctxEnv = v : ctxEnv ctx,
      ctxBinders = Defined : ctxBinders ctx,
      ctxNames = H.idText x : ctxNames ctx,
      ctxTypes = IntMap.insert (H.idUnique x) (ctxLvl ctx, ty) (ctxTypes ctx)
    }

-- | Look up a name, returning the Core reference to use for it and its type.
lookupVar :: Ctx -> H.Ident -> Maybe (Core, VTy)
lookupVar ctx x = do
  (lvl, ty) <- IntMap.lookup (H.idUnique x) (ctxTypes ctx)
  pure (CVar (ctxLvl ctx - lvl - 1) (H.idText x), ty)

-- | The 'Bound' entries of the context as Core variables, in level order
--   (outermost first).  This is the spine a freshly created meta is applied
--   to, which is what keeps metas closed.
boundVarCores :: Ctx -> [Core]
boundVarCores ctx =
  [ CVar (ctxLvl ctx - lvl - 1) name
    | (lvl, bi, name) <-
        zip3
          [0 ..]
          (reverse (ctxBinders ctx))
          (reverse (ctxNames ctx)),
      bi == Bound
  ]
