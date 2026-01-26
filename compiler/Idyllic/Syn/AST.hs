module Idyllic.Syn.AST
  ( Prog,
    Module (..),
    LDecl,
    Decl (..),
    LRecordDef,
    RecordDef (..),
    LClassDef,
    ClassDef (..),
    LClassDecl,
    ClasLDecl (..),
    LInstanceDecl,
    InstanceDecl (..),
    LSig,
    Sig (..),
    Rhs (..),
    LTerm,
    Term (..),
    LBind,
    Bind (..),
    LAlt,
    Alt (..),
    LGuard,
    Guard (..),
    LPat,
    Pat (..),
    Ident (..),
    Lit (..),
  )
where

import Data.Text (Text)
import Idyllic.Utils.Located (Located)

type Prog = Located Module

data Module = Module
  { moduleName :: !Text,
    moduleDecls :: [LDecl]
  }
  deriving (Show, Eq, Ord)

type LDecl = Located Decl

data Decl
  = DeclImport ![Ident]
  | DeclExport !Ident
  | DecLClassDecl LClassDecl
  | DeclClassDef LClassDef
  | DeclInstanceDecl LInstanceDecl
  | DeclRecordDef LRecordDef
  deriving (Show, Eq, Ord)

type LRecordDef = Located RecordDef

data RecordDef = RecordDef
  { recordName :: !Ident,
    recordTypeParams :: ![Ident],
    recordFields :: [(Ident, LTerm)]
  }
  deriving (Show, Eq, Ord)

type LClassDef = Located ClassDef

data ClassDef
  = ClassDef
  { className :: !Ident,
    classSuperclasses :: ![Ident],
    classTypeParams :: ![Ident],
    clasLDecls :: [LClassDecl]
  }
  deriving (Show, Eq, Ord)

type LClassDecl = Located ClasLDecl

data ClasLDecl
  = ClasLDeclSig LSig
  | ClasLDeclBind LBind
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

data Rhs = RhsTerm LTerm | RhsGuard [LGuard]
  deriving (Show, Eq, Ord)

type LTerm = Located Term

data Term
  = TermLit !Lit
  | TermVar !Ident
  | TermApp LTerm LTerm
  | TermLam [LPat] LTerm
  | TermLet [LBind] LTerm
  | TermDo [LTerm]
  | TermNeg LTerm
  | TermBinary !Ident LTerm LTerm
  | TermIf LTerm LTerm LTerm
  | TermMatch LTerm [(LPat, LTerm)]
  | TermList [LTerm]
  | TermTuple [LTerm]
  | TermCons Ident [LTerm]
  | TermRecord (Maybe Ident) [(Ident, LTerm)]
  | TermRecordAccess LTerm Ident
  | TermRecordUpdate LTerm [(Ident, LTerm)]
  | TermUnit
  | Pi [Param] LTerm -- (x : A) -> B, {x : A} -> B, A -> B
  | Ann LTerm LTerm -- e : A  (type annotation)
  | Hole (Maybe Ident) -- _ or ?name for user holes
  | Type
  deriving (Show, Eq, Ord)

data Param = Param
  { paramIdent :: Ident,
    paramType :: LTerm,
    paramImplicit :: Bool
  }
  deriving (Show, Eq, Ord)

type LBind = Located Bind

data Bind
  = BindPat !LPat Rhs [LClassDecl]
  | BindFun !Ident [LAlt] [LClassDecl]
  deriving (Show, Eq, Ord)

type LAlt = Located Alt

data Alt = Alt
  { altPats :: [LPat],
    altTerm :: Rhs
  }
  deriving (Show, Eq, Ord)

type LGuard = Located Guard

data Guard = Guard
  { guardPat :: LPat,
    guardTerm :: LTerm
  }
  deriving (Show, Eq, Ord)

type LPat = Located Pat

data Pat
  = PatWildcard
  | PatLit !Lit
  | PatIdent !Ident
  | PatCons !Ident [LPat]
  | PatAs !Ident LPat
  | PatList [LPat]
  | PatTuple [LPat]
  | PatUnit
  deriving (Show, Eq, Ord)

newtype Ident = Ident (Located Text) deriving (Show, Eq, Ord)

data Lit
  = Int Integer
  | Bool !Bool
  | String !Text
  deriving (Show, Eq, Ord)
