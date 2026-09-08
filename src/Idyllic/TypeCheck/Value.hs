-- | The semantic domain: values, spines and closures.
--
--   This module is deliberately data-only.  Once metavariables enter the
--   picture, evaluating or comparing a value means consulting the
--   metacontext (a solved meta has to be expanded before you can look at the
--   value's head), so 'Idyllic.TypeCheck.Eval.eval', @quote@ and
--   'Idyllic.TypeCheck.Unify.unify' all live in the checker's monad and are
--   defined elsewhere.
--
--   Stuck computations are kept in /head-spine/ form — 'VRigid' for a
--   computation stuck on a bound variable, 'VFlex' for one stuck on an
--   unsolved meta — because pattern unification needs to look at a meta's
--   whole argument list at once.
module Idyllic.TypeCheck.Value
  ( Value (..),
    Spine,
    Closure (..),
    Env,
    VTy,
    vVar,
    vMeta,
  )
where

import Data.Text (Text)
import Idyllic.Builtins (Builtin)
import Idyllic.Syntax.AST (Lit)
import Idyllic.TypeCheck.Core

-- | Values for the variables in scope, innermost first, so a 'Core' de Bruijn
--   index is a direct offset into this list.
type Env = [Value]

-- | Arguments of a stuck application, in application order: @f a b@ has spine
--   @[a, b]@.  Spines are short, so appending at the end is fine.
type Spine = [Value]

-- | A suspended term together with the environment it was found in.  First
--   order (rather than a Haskell function) so values stay 'Show'able and so
--   'Idyllic.TypeCheck.Unify.rename' can go under binders.
data Closure = Closure Env Core
  deriving (Show)

-- | A type is just a value.
type VTy = Value

data Value
  = VType
  | VLitTy Builtin
  | VLit Lit
  | -- | @(x : dom) -> cod@; @cod@ is a closure awaiting the argument.
    VPi Text VTy Closure
  | VLam Text Closure
  | -- | Stuck on an /unsolved/ metavariable.  Becomes reducible the moment
    --   that meta is solved, which is why every value must be @force@d before
    --   its head is inspected.
    VFlex MetaVar Spine
  | -- | Stuck on a bound variable (a de Bruijn level), and therefore stuck
    --   for good.
    VRigid Lvl Text Spine
  | -- | The result of an ill-typed subterm.  Absorbing during unification,
    --   which stops one type error from cascading.
    VError
  deriving (Show)

vVar :: Lvl -> Text -> Value
vVar l n = VRigid l n []

vMeta :: MetaVar -> Value
vMeta m = VFlex m []
