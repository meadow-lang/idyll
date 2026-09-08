-- | Unification, including Miller pattern unification for metavariables.
--
--   This replaces the pure conversion check a meta-free calculus would use:
--   deciding @t == u@ can now /solve/ metas as a side effect, which is
--   exactly how a bidirectional elaborator learns the types it was not told.
--
--   The solvable case is the /pattern fragment/: a meta applied to distinct
--   bound variables, @?m x y z =? t@.  Such an equation has a unique solution
--   @?m := \\x y z. t@, provided
--
--     * @t@ does not itself mention @?m@ (the occurs check), and
--     * every variable free in @t@ is among @x y z@ (the scope check),
--
--   both of which fall out of 'rename', which quotes @t@ through the inverted
--   spine and fails if either condition is violated.  Outside that fragment
--   we simply report a mismatch rather than postponing the constraint.
module Idyllic.TypeCheck.Unify
  ( unify,
  )
where

import Control.Monad (foldM)
import Data.IntMap.Strict (IntMap)
import Data.IntMap.Strict qualified as IntMap
import Data.Text (Text)
import Idyllic.TypeCheck.Core
import Idyllic.TypeCheck.Eval
import Idyllic.TypeCheck.Monad
import Idyllic.TypeCheck.Value

-- | Decide whether two values are convertible, solving metas as needed.
--   Returns 'False' on failure; the caller turns that into a diagnostic, so
--   one failed unification does not abort the whole module.
unify :: Lvl -> Value -> Value -> TC s Bool
unify l t u = do
  t' <- force t
  u' <- force u
  case (t', u') of
    -- an already-reported error unifies with anything, so a single mistake
    -- produces a single message
    (VError, _) -> pure True
    (_, VError) -> pure True
    (VLam _ a, VLam _ b) -> under a b
    -- eta: a lambda equals anything that behaves like it under application
    (VLam _ a, _) -> do
      lhs <- a $$ fresh
      rhs <- vApp u' fresh
      unify (l + 1) lhs rhs
    (_, VLam _ b) -> do
      lhs <- vApp t' fresh
      rhs <- b $$ fresh
      unify (l + 1) lhs rhs
    (VType, VType) -> pure True
    (VLitTy a, VLitTy b) -> pure (a == b)
    (VLit a, VLit b) -> pure (a == b)
    (VPi _ a b, VPi _ a' b') -> do
      ok <- unify l a a'
      if ok then under b b' else pure False
    (VRigid k _ sp, VRigid k' _ sp')
      | k == k' -> unifySpine l sp sp'
    (VFlex m sp, VFlex m' sp')
      | m == m' -> unifySpine l sp sp'
    (VFlex m sp, _) -> solve l m sp u'
    (_, VFlex m sp) -> solve l m sp t'
    _ -> pure False
  where
    fresh = vVar l "_"
    under a b = do
      lhs <- a $$ fresh
      rhs <- b $$ fresh
      unify (l + 1) lhs rhs

unifySpine :: Lvl -> Spine -> Spine -> TC s Bool
unifySpine _ [] [] = pure True
unifySpine l (a : as) (b : bs) = do
  ok <- unify l a b
  if ok then unifySpine l as bs else pure False
unifySpine _ _ _ = pure False

-- ---------------------------------------------------------------------------
-- Pattern unification
-- ---------------------------------------------------------------------------

-- | The inverse of a meta's spine: a map from the levels of the /current/
--   context back to the levels of the meta's own (smaller) context.
data PartialRenaming = PRen
  { -- | size of the meta's context, i.e. the spine length
    prenDom :: Lvl,
    -- | size of the context the right-hand side lives in
    prenCod :: Lvl,
    -- | codomain level to domain level; absent means out of scope
    prenMap :: IntMap Lvl,
    -- | binder names, in domain order, for readable solutions
    prenNames :: [Text]
  }

-- | Go under a binder on both sides.
liftPRen :: Text -> PartialRenaming -> PartialRenaming
liftPRen n (PRen dom cod m names) =
  PRen (dom + 1) (cod + 1) (IntMap.insert cod dom m) (names <> [n])

-- | Invert a meta's spine, if it is in the pattern fragment: every argument
--   must be a distinct bound variable applied to nothing.
invert :: Lvl -> Spine -> TC s (Maybe PartialRenaming)
invert gamma sp = fmap finish <$> foldM step (Just (0, IntMap.empty, [])) sp
  where
    finish (dom, m, names) = PRen dom gamma m names

    step Nothing _ = pure Nothing
    step (Just (dom, m, names)) v = do
      v' <- force v
      pure $ case v' of
        VRigid k n [] | not (IntMap.member k m) ->
          Just (dom + 1, IntMap.insert k dom m, names <> [n])
        -- a repeated variable, or anything that is not a plain variable,
        -- puts this equation outside the pattern fragment
        _ -> Nothing

-- | Quote a value into the meta's context, renaming variables through
--   @pren@.  Returns 'Nothing' if the occurs check or the scope check fails.
rename :: MetaVar -> PartialRenaming -> Value -> TC s (Maybe Core)
rename m = go
  where
    go pren v = do
      v' <- force v
      case v' of
        VType -> pure (Just CType)
        VLitTy b -> pure (Just (CLitTy b))
        VLit l -> pure (Just (CLit l))
        VError -> pure (Just CError)
        VFlex m' sp
          -- occurs check: the solution may not mention the meta being solved
          | m' == m -> pure Nothing
          | otherwise -> goSpine pren (CMeta m') sp
        VRigid k n sp -> case IntMap.lookup k (prenMap pren) of
          -- scope check: this variable is not one the meta may depend on
          Nothing -> pure Nothing
          Just k' -> goSpine pren (CVar (prenDom pren - k' - 1) n) sp
        VLam x cl -> do
          body <- cl $$ vVar (prenCod pren) x
          fmap (CLam x) <$> go (liftPRen x pren) body
        VPi x a cl -> do
          ma <- go pren a
          body <- cl $$ vVar (prenCod pren) x
          mb <- go (liftPRen x pren) body
          pure (CPi x <$> ma <*> mb)

    goSpine pren = foldM step . Just
      where
        step Nothing _ = pure Nothing
        step (Just acc) v = fmap (CApp acc) <$> go pren v

-- | Solve @?m sp =? rhs@.
solve :: Lvl -> MetaVar -> Spine -> Value -> TC s Bool
solve gamma m sp rhs = do
  mpren <- invert gamma sp
  case mpren of
    Nothing -> pure False
    Just pren -> do
      mbody <- rename m pren rhs
      case mbody of
        Nothing -> pure False
        Just body -> do
          -- abstract over the spine: ?m := \x y z. body
          solution <- eval [] (foldr CLam body (prenNames pren))
          solveMeta m solution
          pure True
