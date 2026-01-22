module Idyllic.Syn.AST
  ( Prog,
    Module (..),
    LDecl,
    Decl (..),
    LRecordDef,
    RecordDef (..),
    LClassDef,
    ClassDef (..),
    LClasLDecl,
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
  | DeclClasLDecl LClasLDecl
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
    clasLDecls :: [LClasLDecl]
  }
  deriving (Show, Eq)

type LClasLDecl = Located ClasLDecl

data ClasLDecl
  = ClasLDeclSig LSig
  | ClasLDeclBind LBind
  deriving (Show, Eq)

type LInstanceDecl = Located InstanceDecl

data InstanceDecl = InstanceDecl
  { instanceClass :: !Ident,
    instanceTypeParams :: ![Ident],
    instanceAnn :: LAnn,
    instanceDecls :: [LClasLDecl]
  }
  deriving (Show, Eq)

type LSig = Located Sig

data Sig = Sig
  { sigNames :: ![Ident],
    sigTypeParams :: ![Ident],
    sigAnn :: LAnn
  }
  deriving (Show, Eq)

data Rhs = RhLExpr LExpr | RhLGuard [LGuard]
  deriving (Show, Eq)

type LExpr = Located Expr

data Expr
  = Lit !Lit
  | Var !Ident
  | App LExpr LExpr
  | Lam [LPat] LExpr
  | Let [LBind] LExpr
  | Do [LExpr]
  | Neg LExpr
  | Binary !Ident LExpr LExpr
  | If LExpr LExpr LExpr
  | Match LExpr [(LPat, LExpr)]
  | List [LExpr]
  | Tuple [LExpr]
  | Cons Ident [LExpr]
  | Record (Maybe Ident) [(Ident, LExpr)]
  | RecordAccess LExpr Ident
  | RecordUpdate LExpr [(Ident, LExpr)]
  | Unit
  deriving (Show, Eq)

type LBind = Located Bind

data Bind
  = BindPat !LPat Rhs [LClasLDecl]
  | BindFun !Ident [LAlt] [LClasLDecl]
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
