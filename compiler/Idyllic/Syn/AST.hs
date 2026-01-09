{-# LANGUAGE DeriveDataTypeable #-}

module Idyllic.Syn.AST where

import Control.Lens (Plated (..))
import Control.Lens.Extras (uniplate)
import Data.Data (Data)
import Data.Text (Text)
import Idyllic.Utils.Span (Span)

data SynNode a = SynNode
  { synNodeKind :: a,
    synNodeSpan :: !Span
  }
  deriving (Show, Eq, Ord, Data)

data Module = Module
  { moduleName :: Ident,
    moduleDecls :: [SDecl]
  }
  deriving (Show, Eq, Ord, Data)

type SDecl = SynNode Decl

data Decl
  = DeclClassDecl SWhereDecl
  | DeclClassDef SClassDef
  | DeclInstanceDecl SInstancDecl
  | DeclRecordDef SRecordDef
  deriving (Show, Eq, Ord, Data)

type SRecordDef = SynNode RecordDef

data RecordDef = RecordDef
  { recordName :: !Ident,
    recordTypeParams :: [Ident],
    recordFields :: [(Ident, STerm)]
  }
  deriving (Show, Eq, Ord, Data)

type SClassDef = SynNode ClassDef

data ClassDef
  = ClassDef
  { className :: !Ident,
    classSuperclasses :: [Ident],
    classTypeParams :: [Ident],
    classDecls :: [SClassDecl]
  }
  deriving (Show, Eq, Ord, Data)

type SClassDecl = SynNode ClassDecl

data ClassDecl
  = ClassDeclSig SSig
  | ClassDeclBind FunBind
  deriving (Show, Eq, Ord, Data)

type SWhereDecl = SynNode WhereDecl

data WhereDecl = WhereDecl
  { whereDeclSig :: SSig,
    whereDeclBind :: SBind
  }
  deriving (Show, Eq, Ord, Data)

type SInstancDecl = SynNode InstanceDecl

data InstanceDecl = InstanceDecl
  { instanceClass :: !Ident,
    instanceTypeParams :: [Ident],
    instanceAnn :: STerm,
    instanceDecls :: [SWhereDecl]
  }
  deriving (Show, Eq, Ord, Data)

type SSig = SynNode Sig

data Sig = Sig
  { sigNames :: [Ident],
    sigAnn :: STerm
  }
  deriving (Show, Eq, Ord, Data)

type STerm = SynNode Term

data Term
  = Lit Lit
  | Var Ident
  | Let SBind STerm
  | Lam [SPat] STerm
  | App STerm [STerm]
  | If STerm STerm STerm
  | Match STerm [SAlt]
  | Infix STerm Ident STerm
  | Neg STerm
  | Cons Ident STerm
  | Record [(Ident, STerm)]
  | FieldAccess STerm Ident
  | RecordUpdate STerm [(Ident, STerm)]
  | List [STerm]
  | Vec [STerm]
  | Tuple [STerm]
  | Pi [Param] STerm -- (x : A) -> B, {x : A} -> B, A -> B
  | Ann STerm STerm -- e : A  (type annotation)
  | Hole (Maybe Ident) -- _ or ?name for user holes
  | Type
  | Unit
  deriving (Show, Eq, Ord, Data)

instance Plated STerm where
  plate = uniplate

data Param = Param
  { paramIdent :: Ident,
    paramType :: STerm,
    paramImplicit :: Bool
  }
  deriving (Show, Eq, Ord, Data)

type SBind = SynNode Bind

data Bind
  = BindPat PatBind
  | BindFun FunBind
  deriving (Show, Eq, Ord, Data)

data Rhs = RhsTerm STerm | RhsGuard [SGuard]
  deriving (Show, Eq, Ord, Data)

data PatBind = PatBind
  { patBindPat :: SPat,
    patBindBody :: Rhs,
    patBindWhereDecls :: [SWhereDecl]
  }
  deriving (Show, Eq, Ord, Data)

data FunBind = FunBind
  { funName :: !Ident,
    funAlts :: [SAlt],
    funWhereDecls :: [SWhereDecl]
  }
  deriving (Show, Eq, Ord, Data)

type SAlt = SynNode Alt

data Alt = Alt
  { altPat :: SPat,
    altBody :: Rhs
  }
  deriving (Show, Eq, Ord, Data)

type SGuard = SynNode Guard

data Guard = Guard
  { guardPat :: SPat,
    guardBody :: STerm
  }
  deriving (Show, Eq, Ord, Data)

type SPat = SynNode Pat

data Pat
  = PatWildcard
  | PatLit Lit
  | PatIdent Ident
  | PatCons Ident [SPat]
  | PatAs Ident SPat
  | PatList [SPat]
  | PatTuple [SPat]
  | PatUnit
  deriving (Show, Eq, Ord, Data)

instance Plated SPat where
  plate = uniplate

type Ident = SynNode Text

type SLit = SynNode Lit

data Lit
  = LitInt
  | LitString
  | LitChar
  deriving (Show, Eq, Ord, Data)