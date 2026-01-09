module Idyllic.Syn.AST where

import Data.Text (Text)
import Idyllic.Utils.Span (Span)

data SynNode a = SynNode
  { synNodeKind :: a,
    synNodeSpan :: Span
  }
  deriving (Show, Eq, Ord)

data Module = Module
  { moduleName :: Ident,
    moduleDecls :: [SDecl]
  }
  deriving (Show, Eq, Ord)

type SDecl = SynNode Decl

data Decl
  = DeclFun Ident [SPat] SExpr
  | DeclDef Ident SExpr
  | DeclRecord SRecordDef
  | DeclData SDataDef
  | DeclTypeAlias Ident SAnn
  deriving (Show, Eq, Ord)

type SRecordDef = SynNode RecordDef

data RecordDef = RecordDef
  { recordName :: Ident,
    recordFields :: [(Ident, SAnn)]
  }
  deriving (Show, Eq, Ord)

type SDataDef = SynNode DataDef

data DataDef = DataDef
  { dataName :: Ident,
    dataCons :: [(Ident, [SPat])]
  }
  deriving (Show, Eq, Ord)

type SExpr = SynNode Expr

data Expr
  = ExprLit Lit
  | ExprIdent Ident
  | ExprApp SExpr [SExpr]
  | ExprLam [SPat] SExpr
  | ExprLet Bind SExpr
  | ExprIf SExpr SExpr SExpr
  | ExprMatch SExpr [(SPat, SExpr)]
  | ExprAnn SExpr SExpr
  | ExprCons Ident [SExpr]
  | ExprRecord [(Ident, SExpr)]
  | ExprFieldAccess SExpr Ident
  | ExprList [SExpr]
  | ExprVec [SExpr]
  | ExprTuple [SExpr]
  | ExprRef SExpr
  | ExprUnit
  deriving (Show, Eq, Ord)

data Bind = BindFun FunBind | BindPattern PatBind
  deriving (Show, Eq, Ord)

data FunBind = FunBind Ident [SPat] SExpr
  deriving (Show, Eq, Ord)

data PatBind = PatBind Pat SExpr
  deriving (Show, Eq, Ord)

type SAnn = SynNode Ann

data Ann
  = AnnIdent Ident
  | AnnFun SAnn SAnn
  | AnnList SAnn
  | AnnTuple [SAnn]
  | AnnUnit
  deriving (Show, Eq, Ord)

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
  deriving (Show, Eq, Ord)

type Ident = SynNode Text

type SLit = SynNode Lit

data Lit
  = LitInt
  | LitFloat
  | LitString
  | LitChar
  deriving (Show, Eq, Ord)
