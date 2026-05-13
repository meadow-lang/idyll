module Idyllic.AST
  ( Prog,
    Module (..),
    LRecord,
    Record (..),
    LBind,
    Bind (..),
    LTerm,
    Term (..),
    LParam,
    Param (..),
    ParamKind (..),
    LAlt,
    Alt (..),
    LGuard,
    Guard (..),
    LPat,
    Pat (..),
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

data Decl
  = DeclBind LBind
  | DeclRecord Record
  deriving (Show, Eq, Ord)

type LRecord = Located Record

data Record = Record
  { recordName :: Ident,
    recordFields :: [(Ident, LTerm)]
  }
  deriving (Show, Eq, Ord)

type LBind = Located Bind

data Bind
  = BindPat LPat LTerm
  | BindFun Ident [LPat] LTerm
  deriving (Show, Eq, Ord)

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

type LTerm = Located Term

data Term
  = TermLit Lit
  | TermVar Ident
  | TermLet [LBind] LTerm
  | TermLam [LPat] LTerm
  | TermApp LTerm [LTerm]
  | TermIf LTerm LTerm LTerm
  | TermCase LTerm [LAlt]
  | TermInfix LTerm Ident LTerm
  | TermNeg LTerm
  | TermDo [LDoBinding] LTerm
  | TermCons Ident LTerm
  | TermRecord (Maybe Ident) [(Ident, LTerm)]
  | TermFieldAccess LTerm Ident
  | TermRecordUpdate LTerm [(Ident, LTerm)]
  | TermList [LTerm]
  | TermVec [LTerm]
  | TermTuple [LTerm]
  | TermPi [LParam] LTerm -- (x : a) -> b, {x : a} -> b, a -> b
  | TermAnn LTerm LTerm -- e : a  (type annotation)
  | TermRecordSig RecordSig -- {x : a | r}
  | TermHole (Maybe Ident) -- _ or ?name for user holes
  | TermUniverse Int -- Type 0, Type 1, etc.
  | TermUnit
  deriving (Show, Eq, Ord)

type LDoBinding = Located DoBinding

data DoBinding
  = DoBind Ident LTerm -- x <- action
  | DoLet [LBind] -- let ... in ...
  deriving (Show, Eq, Ord)

type LParam = Located Param

data Param = Param
  { paramKind :: ParamKind,
    paramImplicit :: Bool
  }
  deriving (Show, Eq, Ord)

data ParamKind
  = ParamAnon Ident -- a
  | ParamIdent Ident LTerm -- (x : a)
  | ParamRecord RecordSig -- {x : a | r}
  deriving (Show, Eq, Ord)

data RecordSig = RecordSig
  { recordSigFields :: [(Ident, LTerm)],
    recordSigRow :: Maybe Ident
  }
  deriving (Show, Eq, Ord)

-- instance Plated LTerm where
--   plate = uniplate

type LPat = Located Pat

data Pat
  = PatWildcard
  | PatLit Lit
  | PatIdent Ident
  | PatCons Ident [LPat]
  | PatAs Ident LPat
  | PatList [LPat]
  | PatTuple [LPat]
  | PatRecord (Maybe Ident) [(Ident, LPat)]
  | PatUnit
  deriving (Show, Eq, Ord)

type Ident = Located Text

data Lit
  = LitInt Integer
  | LitBool Bool
  | LitString Text
  deriving (Show, Eq, Ord)
