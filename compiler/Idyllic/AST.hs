module Idyllic.AST
  ( Prog,
    Module (..),
    LDecl,
    Decl (..),
    LRecord,
    LClassDef,
    LClassDecl,
    LInstanceDecl,
    LSig,
    Sig (..),
    Record (..),
    LBind,
    Bind (..),
    Rhs (..),
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
    moduleDecls :: [LDecl]
  }
  deriving (Show, Eq, Ord)

type LDecl = Located Decl

data Decl
  = DeclUse ![Ident]
  | DeclExport !Ident
  | DeclClassDecl LClassDecl
  | DeclClassDef LClassDef
  | DeclInstanceDecl LInstanceDecl
  | DeclRecordDef LRecord
  deriving (Show, Eq, Ord)

type LRecord = Located Record

data Record = Record
  { recordName :: Ident,
    recordFields :: [(Ident, LTerm)]
  }
  deriving (Show, Eq, Ord)

type LClassDef = Located ClassDef

data ClassDef
  = ClassDef
  { className :: !Ident,
    classSuperclasses :: ![Ident],
    classTypeParams :: ![Ident],
    classDecls :: [LClassDecl]
  }
  deriving (Show, Eq, Ord)

type LClassDecl = Located ClassDecl

data ClassDecl
  = ClassDeclSig LSig
  | ClassDeclBind LBind
  deriving (Show, Eq, Ord)

type LInstanceDecl = Located InstanceDecl

data InstanceDecl = InstanceDecl
  { instanceClass :: !Ident,
    instanceTypeParams :: ![Ident],
    instanceAnn :: LTerm,
    instanceDecls :: [LClassDecl]
  }
  deriving (Show, Eq, Ord)

type LSig = Located Sig

data Sig = Sig
  { sigNames :: ![Ident],
    sigTypeParams :: ![Ident],
    sigAnn :: LTerm
  }
  deriving (Show, Eq, Ord)

type LBind = Located Bind

data Bind
  = BindPat LPat Rhs
  | BindFun Ident [LPat] LTerm
  deriving (Show, Eq, Ord)

data Rhs = RhsTerm LTerm | RhsGuard [LGuard]
  deriving (Show, Eq, Ord)

type LAlt = Located Alt

data Alt = Alt
  { altPat :: [LPat],
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
  | TermPi [LParam] LTerm -- (x : a) -> b, (?x : a) -> b, a -> b
  | TermAnn LTerm LTerm -- e : a  (type annotation)
  | TermRecordSig RecordSig -- {x : a | r}
  | TermHole (Maybe Ident) -- _ or ?name for user holes
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
