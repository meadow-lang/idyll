module Idyllic.AST
  ( Prog,
    Module (..),
    LBind,
    Bind (..),
    LTerm,
    Term (..),
    Ident,
    Lit (..),
  )
where

import Data.Text (Text)
import Idyllic.Utils (Located)

type Prog = Located Module

data Module = Module
  { moduleName :: Text,
    moduleDecls :: [LBind]
  }
  deriving (Show, Eq, Ord)

type LBind = Located Bind

data Bind
  = BindName Ident LTerm
  | BindFun Ident [Ident] LTerm
  deriving (Show, Eq, Ord)

type LTerm = Located Term

data Rhs = RhsTerm LTerm | RhsGuard [LGuard]
  deriving (Show, Eq, Ord)

type LAlt = Located Alt

data Alt = Alt
  { altPat :: LPat,
    altBody :: Rhs
  }
  deriving (Show, Eq, Ord)

type LGuard = Located Guard

data Guard = Guard
  { guardPat :: LPat,
    guardBody :: LTerm
  }
  deriving (Show, Eq, Ord)

data Term
  = Lit Lit
  | Var Ident
  | Let LBind LTerm
  | Lam [LPat] LTerm
  | App LTerm [LTerm]
  | If LTerm LTerm LTerm
  | Case LTerm [LAlt]
  | Infix LTerm Ident LTerm
  | Neg LTerm
  | Cons Ident LTerm
  | Record [(Ident, LTerm)]
  | FieldAccess LTerm Ident
  | RecordUpdate LTerm [(Ident, LTerm)]
  | List [LTerm]
  | Vec [LTerm]
  | Tuple [LTerm]
  | Pi [Param] LTerm -- (x : A) -> B, {x : A} -> B, A -> B
  | Ann LTerm LTerm -- e : A  (type annotation)
  | Hole (Maybe Ident) -- _ or ?name for user holes
  | Type
  | Unit
  deriving (Show, Eq, Ord)

data Param = Param
  { paramIdent :: Ident,
    paramType :: LTerm,
    paramImplicit :: Bool
  }
  deriving (Show, Eq, Ord)

-- instance Plated LTerm where
--   plate = uniplate

type Ident = Located Text

type LPat = Located Pat

data Pat
  = PatWildcard
  | PatLit Lit
  | PatIdent Ident
  | PatCons Ident [LPat]
  | PatAs Ident LPat
  | PatList [LPat]
  | PatTuple [LPat]
  | PatUnit
  deriving (Show, Eq, Ord)

data Lit
  = LitInt Integer
  | LitBool Bool
  | LitString Text
  deriving (Show, Eq, Ord)
