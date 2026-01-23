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
    LExpr,
    Expr (..),
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
  deriving (Show, Eq)

type LDecl = Located Decl

data Decl
  = DeclImport ![Ident]
  | DeclExport !Ident
  | DecLClassDecl LClassDecl
  | DeclClassDef LClassDef
  | DeclInstanceDecl LInstanceDecl
  | DeclRecordDef LRecordDef
  deriving (Show, Eq)

type LRecordDef = Located RecordDef

data RecordDef = RecordDef
  { recordName :: !Ident,
    recordTypeParams :: ![Ident],
    recordFields :: [(Ident, LAnn)]
  }
  deriving (Show, Eq)

type LClassDef = Located ClassDef

data ClassDef
  = ClassDef
  { className :: !Ident,
    classSuperclasses :: ![Ident],
    classTypeParams :: ![Ident],
    clasLDecls :: [LClassDecl]
  }
  deriving (Show, Eq)

type LClassDecl = Located ClasLDecl

data ClasLDecl
  = ClasLDeclSig LSig
  | ClasLDeclBind LBind
  deriving (Show, Eq)

type LInstanceDecl = Located InstanceDecl

data InstanceDecl = InstanceDecl
  { instanceClass :: !Ident,
    instanceTypeParams :: ![Ident],
    instanceAnn :: LAnn,
    instanceDecls :: [LClassDecl]
  }
  deriving (Show, Eq)

type LSig = Located Sig

data Sig = Sig
  { sigNames :: ![Ident],
    sigTypeParams :: ![Ident],
    sigAnn :: LAnn
  }
  deriving (Show, Eq)

data Rhs = RhsExpr LExpr | RhsGuard [LGuard]
  deriving (Show, Eq)

type LExpr = Located Expr

data Expr
  = ExprLit !Lit
  | ExprVar !Ident
  | ExprApp LExpr LExpr
  | ExprLam [LPat] LExpr
  | ExprLet [LBind] LExpr
  | ExprDo [LExpr]
  | ExprNeg LExpr
  | ExprBinary !Ident LExpr LExpr
  | ExprIf LExpr LExpr LExpr
  | ExprMatch LExpr [(LPat, LExpr)]
  | ExprList [LExpr]
  | ExprTuple [LExpr]
  | ExprCons Ident [LExpr]
  | ExprRecord (Maybe Ident) [(Ident, LExpr)]
  | ExprRecordAccess LExpr Ident
  | ExprRecordUpdate LExpr [(Ident, LExpr)]
  | ExprUnit
  deriving (Show, Eq)

type LBind = Located Bind

data Bind
  = BindPat !LPat Rhs [LClassDecl]
  | BindFun !Ident [LAlt] [LClassDecl]
  deriving (Show, Eq)

type LAlt = Located Alt

data Alt = Alt
  { altPats :: [LPat],
    altExpr :: Rhs
  }
  deriving (Show, Eq)

type LGuard = Located Guard

data Guard = Guard
  { guardPat :: LPat,
    guardExpr :: LExpr
  }
  deriving (Show, Eq)

type LAnn = Located Ann

data Ann
  = AnnVar !Ident
  | AnnIdent !Ident
  | AnnFun !LAnn !LAnn
  | AnnList !LAnn
  | AnnTuple [LAnn]
  | AnnUnit
  deriving (Show, Eq)

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
  deriving (Show, Eq)

newtype Ident = Ident (Located Text) deriving (Show, Eq, Ord)

data Lit
  = Int Integer
  | Bool !Bool
  | String !Text
  deriving (Show, Eq)
