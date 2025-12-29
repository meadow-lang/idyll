module Idyllic.Syn.AST where

import Control.Lens (Plated (..))
import Data.Text (Text)
import Idyllic.Utils.Span (Span)

data SynNode a = SynNode
  { synNodeKind :: a,
    synNodeSpan :: !Span
  }
  deriving (Show, Eq, Ord)

data Module = Module
  { moduleName :: Ident,
    moduleDecls :: [SDecl]
  }
  deriving (Show, Eq, Ord)

type SDecl = SynNode Decl

data Decl
  = DeclClassDecl SWhereDecl
  | DeclClassDef SClassDef
  | DeclInstanceDecl SInstancDecl
  | DeclRecordDef SRecordDef
  deriving (Show, Eq, Ord)

type SRecordDef = SynNode RecordDef

data RecordDef = RecordDef
  { recordName :: !Ident,
    recordTypeParams :: [Ident],
    recordFields :: [(Ident, STerm)]
  }
  deriving (Show, Eq, Ord)

type SClassDef = SynNode ClassDef

data ClassDef
  = ClassDef
  { className :: !Ident,
    classSuperclasses :: [Ident],
    classTypeParams :: [Ident],
    classDecls :: [SClassDecl]
  }
  deriving (Show, Eq, Ord)

type SClassDecl = SynNode ClassDecl

data ClassDecl
  = ClassDeclSig SSig
  | ClassDeclBind FunBind
  deriving (Show, Eq, Ord)

type SWhereDecl = SynNode WhereDecl

data WhereDecl = WhereDecl
  { whereDeclSig :: SSig,
    whereDeclBind :: SBind
  }
  deriving (Show, Eq, Ord)

type SInstancDecl = SynNode InstanceDecl

data InstanceDecl = InstanceDecl
  { instanceClass :: !Ident,
    instanceTypeParams :: [Ident],
    instanceAnn :: STerm,
    instanceDecls :: [SWhereDecl]
  }
  deriving (Show, Eq, Ord)

type SSig = SynNode Sig

data Sig = Sig
  { sigNames :: [Ident],
    sigAnn :: STerm
  }
  deriving (Show, Eq, Ord)

type STerm = SynNode Term

data Term
  = Lit Lit
  | Var Ident
  | Let SBind STerm
  | If STerm STerm STerm
  | Lam [SPat] STerm
  | App STerm [STerm]
  | Match STerm [SAlt]
  | Infix STerm Ident STerm
  | Neg STerm
  | Cons Ident STerm
  | Pi [Param] STerm -- (x : A) -> B, {x : A} -> B, A -> B
  | Ann STerm STerm -- e : A  (type annotation)
  | Hole (Maybe Ident) -- _ or ?name for user holes
  | Type
  deriving (Show, Eq, Ord)

instance Plated STerm where
  plate _ (SynNode (Lit l) s) = pure (SynNode (Lit l) s)
  plate _ (SynNode (Var x) s) = pure (SynNode (Var x) s)
  plate f (SynNode (Let b body) s) = SynNode <$> (Let <$> traverseBind f b <*> f body) <*> pure s
  plate f (SynNode (If cond thenT elseT) s) = SynNode <$> (If <$> f cond <*> f thenT <*> f elseT) <*> pure s
  plate f (SynNode (Lam pats body) s) = (SynNode . Lam pats <$> f body) <*> pure s
  plate f (SynNode (App fn args) s) = SynNode <$> (App <$> f fn <*> traverse f args) <*> pure s
  plate f (SynNode (Match scrut alts) s) = SynNode <$> (Match <$> f scrut <*> traverse (traverseAlt f) alts) <*> pure s
  plate f (SynNode (Infix left op right) s) = SynNode <$> (Infix <$> f left <*> pure op <*> f right) <*> pure s
  plate f (SynNode (Neg t) s) = (SynNode . Neg <$> f t) <*> pure s
  plate f (SynNode (Cons con t) s) = (SynNode . Cons con <$> f t) <*> pure s
  plate f (SynNode (Pi params body) s) = (SynNode . Pi params <$> f body) <*> pure s
  plate f (SynNode (Ann t ann) s) = SynNode <$> (Ann <$> f t <*> f ann) <*> pure s
  plate _ (SynNode (Hole mi) s) = pure (SynNode (Hole mi) s)
  plate _ (SynNode Type s) = pure (SynNode Type s)

data Param = Param
  { paramIdent :: Ident,
    paramType :: STerm,
    paramImplicit :: Bool
  }
  deriving (Show, Eq, Ord)

type SBind = SynNode Bind

data Bind
  = BindPat PatBind
  | BindFun FunBind
  deriving (Show, Eq, Ord)

traverseBind :: (Applicative f) => (STerm -> f STerm) -> SBind -> f SBind
traverseBind f (SynNode (BindPat pb) s) = (SynNode . BindPat <$> traversePatBind f pb) <*> pure s
traverseBind f (SynNode (BindFun fb) s) = (SynNode . BindFun <$> traverseFunBind f fb) <*> pure s

data Rhs = RhsTerm STerm | RhsGuard [SGuard]
  deriving (Show, Eq, Ord)

traverseRhs :: (Applicative f) => (STerm -> f STerm) -> Rhs -> f Rhs
traverseRhs f (RhsTerm t) = RhsTerm <$> f t
traverseRhs f (RhsGuard guards) = RhsGuard <$> traverse (traverseGuard f) guards

data PatBind = PatBind
  { patBindPat :: SPat,
    patBindBody :: Rhs,
    patBindWhereDecls :: [SWhereDecl]
  }
  deriving (Show, Eq, Ord)

traversePatBind :: (Applicative f) => (STerm -> f STerm) -> PatBind -> f PatBind
traversePatBind f (PatBind pat body whereDecls) = PatBind pat <$> traverseRhs f body <*> pure whereDecls

data FunBind = FunBind
  { funName :: !Ident,
    funAlts :: [SAlt],
    funWhereDecls :: [SWhereDecl]
  }
  deriving (Show, Eq, Ord)

traverseFunBind :: (Applicative f) => (STerm -> f STerm) -> FunBind -> f FunBind
traverseFunBind f (FunBind name alts whereDecls) = FunBind name <$> traverse (traverseAlt f) alts <*> pure whereDecls

type SAlt = SynNode Alt

data Alt = Alt
  { altPat :: SPat,
    altBody :: Rhs
  }
  deriving (Show, Eq, Ord)

traverseAlt :: (Applicative f) => (STerm -> f STerm) -> SAlt -> f SAlt
traverseAlt f (SynNode (Alt pat body) s) = (SynNode . Alt pat <$> traverseRhs f body) <*> pure s

type SGuard = SynNode Guard

data Guard = Guard
  { guardPat :: SPat,
    guardBody :: STerm
  }
  deriving (Show, Eq, Ord)

traverseGuard :: (Applicative f) => (STerm -> f STerm) -> SGuard -> f SGuard
traverseGuard f (SynNode (Guard pat body) s) = (SynNode . Guard pat <$> f body) <*> pure s

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

instance Plated SPat where
  plate _ (SynNode PatWildcard s) = pure (SynNode PatWildcard s)
  plate _ (SynNode (PatLit l) s) = pure (SynNode (PatLit l) s)
  plate _ (SynNode (PatIdent x) s) = pure (SynNode (PatIdent x) s)
  plate f (SynNode (PatCons con pats) s) = (SynNode . PatCons con <$> traverse f pats) <*> pure s
  plate f (SynNode (PatAs x pat) s) = (SynNode . PatAs x <$> f pat) <*> pure s
  plate f (SynNode (PatList pats) s) = (SynNode . PatList <$> traverse f pats) <*> pure s
  plate f (SynNode (PatTuple pats) s) = (SynNode . PatTuple <$> traverse f pats) <*> pure s
  plate _ (SynNode PatUnit s) = pure (SynNode PatUnit s)

type Ident = SynNode Text

type SLit = SynNode Lit

data Lit
  = LitInt
  | LitString
  | LitChar
  deriving (Show, Eq, Ord)
