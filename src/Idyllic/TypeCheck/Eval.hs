-- | Normalisation by evaluation, in the checker's monad.
--
--   These would be pure functions in a calculus without metavariables.  They
--   are monadic here because solving a meta must retroactively change what
--   every value that mentions it /means/: a 'VFlex' whose meta has since been
--   solved is no longer stuck.  'force' is what makes that visible, and every
--   place that inspects a value's head must go through it first.
module Idyllic.TypeCheck.Eval
  ( force,
    eval,
    quote,
    quoteAt,
    nf,
    vApp,
    vAppSpine,
    ($$),
  )
where

import Control.Monad (foldM)
import Idyllic.TypeCheck.Core
import Idyllic.TypeCheck.Monad
import Idyllic.TypeCheck.Value

-- | Expand the head of a value until it is no longer a solved metavariable.
--   Every @case@ on a 'Value' in the checker forces first.
force :: Value -> TC s Value
force v@(VFlex m sp) = do
  entry <- lookupMeta m
  case entry of
    Solved sol _ -> force =<< vAppSpine sol sp
    Unsolved _ -> pure v
force v = pure v

-- | Instantiate a closure.
($$) :: Closure -> Value -> TC s Value
Closure env body $$ v = eval (v : env) body

infixl 9 $$

vApp :: Value -> Value -> TC s Value
vApp f a = case f of
  VLam _ cl -> cl $$ a
  VFlex m sp -> pure (VFlex m (sp <> [a]))
  VRigid l n sp -> pure (VRigid l n (sp <> [a]))
  -- applying a non-function: already reported, keep going
  _ -> pure VError

vAppSpine :: Value -> Spine -> TC s Value
vAppSpine = foldM vApp

-- | Evaluate a Core term in an environment.  Total: applying a non-function
--   yields 'VError' rather than crashing, because the checker keeps going
--   after reporting an error.
eval :: Env -> Core -> TC s Value
eval env = \case
  CType -> pure VType
  CLitTy b -> pure (VLitTy b)
  CLit l -> pure (VLit l)
  CError -> pure VError
  CVar i _ -> pure $ case drop i env of
    (v : _) -> v
    [] -> VError
  CMeta m -> do
    entry <- lookupMeta m
    pure $ case entry of
      Solved sol _ -> sol
      Unsolved _ -> vMeta m
  CPi x a b -> do
    a' <- eval env a
    pure (VPi x a' (Closure env b))
  CLam x b -> pure (VLam x (Closure env b))
  CApp f a -> do
    f' <- eval env f
    a' <- eval env a
    vApp f' a'

-- | Read a value back into Core.  @l@ is the size of the context the result
--   will live in.  Solved metas are expanded on the way, so quoting doubles
--   as zonking: what comes out mentions only genuinely unsolved metas.
quote :: Lvl -> Value -> TC s Core
quote l v = do
  v' <- force v
  case v' of
    VType -> pure CType
    VLitTy b -> pure (CLitTy b)
    VLit x -> pure (CLit x)
    VError -> pure CError
    VFlex m sp -> quoteSpine l (CMeta m) sp
    VRigid k n sp -> quoteSpine l (CVar (l - k - 1) n) sp
    VPi x a b -> do
      a' <- quote l a
      b' <- quote (l + 1) =<< b $$ vVar l x
      pure (CPi x a' b')
    VLam x b -> do
      b' <- quote (l + 1) =<< b $$ vVar l x
      pure (CLam x b')

quoteSpine :: Lvl -> Core -> Spine -> TC s Core
quoteSpine l = foldM (\acc v -> CApp acc <$> quote l v)

quoteAt :: Ctx -> Value -> TC s Core
quoteAt ctx = quote (ctxLvl ctx)

-- | Normalise a Core term in place.
nf :: Lvl -> Env -> Core -> TC s Core
nf l env c = quote l =<< eval env c
