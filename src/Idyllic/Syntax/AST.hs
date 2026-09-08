module Idyllic.Syntax.AST where

import Data.Text (Text)
import Idyllic.Utils (Located)

-- | A whole program is, for now, a single term (REPL / demo oriented).
type Prog = LTerm

type LTerm = Located Term

-- | The core calculus: a Calculus-of-Constructions style dependent lambda
--   calculus.  Application and the binders are curried (one binder / one
--   argument per node); the parser desugars the surface telescopes into
--   these.
data Term
  = -- | The sort @Type@.  A single universe is enough for the demo; a level
    --   field can be added here later without touching the parser much.
    TermType
  | TermVar Ident
  | TermLit Lit
  | -- | Dependent function type.  @(x : a) -> b@ is @TermPi (Just x) a b@;
    --   the non-dependent arrow @a -> b@ is @TermPi Nothing a b@.  Following
    --   Haskell conventions, binders are lower-case names.
    TermPi (Maybe Ident) LTerm LTerm
  | -- | Lambda abstraction, @\\x -> e@ or @\\(x : a) -> e@.
    TermLam LParam LTerm
  | -- | Application, @f a@.
    TermApp LTerm LTerm
  | -- | Type annotation, @e : a@.
    TermAnn LTerm LTerm
  | -- | An explicit hole, @_@: elaborates to a fresh metavariable.
    TermHole
  | TermLet [LBind] LTerm
  deriving (Show, Eq, Ord)

type Ident = Located Text

type LParam = Located Param

-- | A binder: a (lower-case) name and an optional type annotation (@x@ or
--   @(x : a)@).
data Param = Param Ident (Maybe LTerm)
  deriving (Show, Eq, Ord)

type LBind = Located Bind

data Bind
  = -- | @x : a = e@ or @x = e@.
    BindName Ident (Maybe LTerm) LTerm
  | -- | @f (x : a) y : b = e@ (sugar for a lambda / Pi).
    BindFun Ident [LParam] (Maybe LTerm) LTerm
  deriving (Show, Eq, Ord)

data Lit
  = LitUnit
  | LitInt Integer
  | LitBool Bool
  | LitString Text
  deriving (Show, Eq, Ord)
