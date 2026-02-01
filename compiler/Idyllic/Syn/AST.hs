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
    ClassDecl (..),
    LInstanceDecl,
    InstanceDecl (..),
    LSig,
    Sig (..),
    Rhs (..),
    LExpr,
    Expr (..),
    LBinOp,
    BinOp (..),
    LBind,
    Bind (..),
    LAlt,
    Alt (..),
    LGuard,
    Guard (..),
    LAnn,
    Ann (..),
    LPat,
    Pat (..),
    Ident,
    Lit (..),
  )
where

import Data.Text (Text)
import Idyllic.Utils.Located (Located)

type Prog = Located Module

data Module = Module
  { moduleName :: Text,
    moduleDecls :: [LDecl]
  }
  deriving (Show, Eq, Ord)

type LDecl = Located Decl

data Decl
  = DeclImport [Ident]
  | DeclExport Ident
  | DeclClassDecl LClassDecl
  | DeclClassDef LClassDef
  | DeclInstanceDecl LInstanceDecl
  | DeclRecordDef LRecordDef
  deriving (Show, Eq, Ord)

type LRecordDef = Located RecordDef

data RecordDef = RecordDef
  { recordName :: Ident,
    recordTypeParams :: [Ident],
    recordFields :: [(Ident, LExpr)]
  }
  deriving (Show, Eq, Ord)

type LClassDef = Located ClassDef

data ClassDef
  = ClassDef
  { className :: Ident,
    classSuperclasses :: [Ident],
    classTypeParams :: [Ident],
    clasLDecls :: [LClassDecl]
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
    instanceAnn :: LExpr,
    instanceDecls :: [LClassDecl]
  }
  deriving (Show, Eq, Ord)

type LSig = Located Sig

data Sig = Sig
  { sigNames :: [Ident],
    sigTypeParams :: [Ident],
    sigAnn :: LAnn
  }
  deriving (Show, Eq, Ord)

data Rhs = RhsExpr LExpr | RhsGuard [LGuard]
  deriving (Show, Eq, Ord)

type LExpr = Located Expr

data Expr
  = ExprLit Lit
  | ExprVar Ident
  | ExprApp LExpr [LExpr]
  | ExprLam [LPat] LExpr
  | ExprLet [LBind] LExpr
  | ExprDo [LExpr]
  | ExprNeg LExpr
  | ExprBinary LBinOp LExpr LExpr
  | ExprIf LExpr LExpr LExpr
  | ExprCase LExpr [(LPat, LExpr)]
  | ExprList [LExpr]
  | ExprTuple [LExpr]
  | ExprCons Ident [LExpr]
  | ExprRecord Ident [(Ident, LExpr)]
  | ExprRecordAccess LExpr Ident
  | ExprRecordUpdate LExpr [(Ident, LExpr)]
  | ExprAnn LExpr LAnn
  | ExprUnit
  | Type
  deriving (Show, Eq, Ord)

type LBinOp = Located BinOp

data BinOp
  = BinOpAdd
  | BinOpSub
  | BinOpMul
  | BinOpDiv
  | BinOpMod
  | BinOpAnd
  | BinOpOr
  | BinOpEq
  | BinOpNeq
  | BinOpLt
  | BinOpLte
  | BinOpGt
  | BinOpGte
  deriving (Show, Eq, Ord)

type LBind = Located Bind

data Bind
  = BindPat !LPat Rhs [LClassDecl]
  | BindFun !Ident [LAlt] [LClassDecl]
  deriving (Show, Eq, Ord)

type LAlt = Located Alt

data Alt = Alt
  { altPats :: [LPat],
    altExpr :: Rhs
  }
  deriving (Show, Eq, Ord)

type LGuard = Located Guard

data Guard = Guard
  { guardPat :: LPat,
    guardExpr :: LExpr
  }
  deriving (Show, Eq, Ord)

type LAnn = Located Ann

data Ann
  = AnnVar Ident
  | AnnIdent Ident
  | AnnFun LAnn LAnn
  | AnnList LAnn
  | AnnTuple [LAnn]
  | AnnUnit
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
  | PatUnit
  deriving (Show, Eq, Ord)

type Ident = Located Text

data Lit
  = Int Integer
  | Bool Bool
  | String Text
  deriving (Show, Eq, Ord)
