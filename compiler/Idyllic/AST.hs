module Idyllic.AST where

--   ( Prog,
--     Module (..),
--     LDecl,
--     Decl (..),
--     LRecord,
--     LDataDef,
--     DataDef (..),
--     DataCons (..),
--     LClassDef,
--     LClassDecl,
--     LInstanceDecl,
--     LSig,
--     Sig (..),
--     Record (..),
--     LBind,
--     Bind (..),
--     Rhs (..),
--     LTerm,
--     Term (..),
--     LParam,
--     Param (..),
--     ParamKind (..),
--     LAlt,
--     Alt (..),
--     LGuardedTerm,
--     GuardedTerm (..),
--     LGuardStmt,
--     GuardStmt (..),
--     LPat,
--     Pat (..),
--     Ident,
--     Lit (..),
--   )
-- where

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
  = DeclUse [Ident]
  | DeclExport Ident
  | DeclClassDecl LClassDecl
  | DeclClassDef LClassDef
  | DeclInstanceDecl LInstanceDecl
  | DeclRecordDef LRecord
  | DeclDataDef LDataDef
  deriving (Show, Eq, Ord)

type LRecord = Located Record

data Record = Record
  { recordName :: Ident,
    recordFields :: [(Ident, LTerm)]
  }
  deriving (Show, Eq, Ord)

type LDataDef = Located DataDef

data DataDef = DataDef
  { dataName :: Ident,
    dataConstructors :: [DataCons]
  }
  deriving (Show, Eq, Ord)

data DataCons
  = DataConsRecord {dataConsName :: Ident, dataConsRecordFields :: [(Ident, LTerm)]}
  | DataConsAnon {dataConsName :: Ident, dataConsArgs :: [LTerm]}
  deriving (Show, Eq, Ord)

type LClassDef = Located ClassDef

data ClassDef
  = ClassDef
  { className :: Ident,
    classSuperclasses :: [Ident],
    classTypeParams :: [Ident],
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
  { instanceClass :: Ident,
    instanceTypeParams :: [Ident],
    instanceAnn :: LTerm,
    instanceDecls :: [LClassDecl]
  }
  deriving (Show, Eq, Ord)

type LSig = Located Sig

data Sig = Sig
  { sigNames :: [Ident],
    sigTypeParams :: [Ident],
    sigAnn :: LTerm
  }
  deriving (Show, Eq, Ord)

type LBind = Located Bind

data Bind
  = BindPat LPat Rhs
  | BindFun Ident [LPat] Rhs
  deriving (Show, Eq, Ord)

data Rhs = RhsTerm LTerm | RhsGuard [LGuardedTerm]
  deriving (Show, Eq, Ord)

type LAlt = Located Alt

data Alt = Alt
  { altPat :: [LPat],
    altBody :: Rhs
  }
  deriving (Show, Eq, Ord)

type LGuardedTerm = Located GuardedTerm

data GuardedTerm = GuardedTerm
  { guardStmts :: [LGuardStmt], -- the conditions, comma-separated
    guardResult :: LTerm -- the = result
  }
  deriving (Show, Eq, Ord)

type LGuardStmt = Located GuardStmt

data GuardStmt
  = GuardBool LTerm -- x > 0
  | GuardPat LPat LTerm -- Just y <- f x
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
  | TermRecord (Maybe Ident) [(Ident, LTerm)]
  | TermSel LTerm Ident
  | TermRecordUpdate LTerm [(Ident, LTerm)]
  | TermList [LTerm]
  | TermVec [LTerm]
  | TermTuple [LTerm]
  | TermPi [LParam] LTerm -- (x : a) -> b, (?x : a) -> b, a -> b
  | TermAnn LTerm LTerm -- e : a  (type annotation)
  | TermRecordSig RecordSig -- {x : a | r}
  | TermHole (Maybe Ident) -- _ or _name for user holes
  | TermType -- TypeInType
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
    paramImplicit :: Plicity
  }
  deriving (Show, Eq, Ord)

data ParamKind
  = ParamAnon LTerm -- a, Vec n a, etc.
  | ParamIdent Ident LTerm -- (x : a)
  | ParamRecord RecordSig -- {x : a | r}
  deriving (Show, Eq, Ord)

data Plicity
  = Explicit
  | Implicit
  | AutoImplicit
  | Constraint
  deriving (Show, Eq, Ord)

data RecordSig = RecordSig
  { recordSigFields :: [(Ident, LTerm)],
    recordSigRow :: Maybe Ident
  }
  deriving (Show, Eq, Ord)

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

type Ident = Located Name

data Name = Name
  { nameKind :: NameKind,
    nameText :: Text
  }
  deriving (Show, Eq, Ord)

data NameKind
  = NameLower
  | NameUpper
  deriving (Show, Eq, Ord)

data Lit
  = LitInt Integer
  | LitString Text
  deriving (Show, Eq, Ord)
