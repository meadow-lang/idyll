module Idyllic.HIR
  ( Prog,
    Module (..),
    LBind,
    Bind (..),
    ExprId,
    Expr (..),
    Ident,
    Lit (..),
  )
where

import Data.Text (Text)
import Idyllic.Utils (Located)

type NodeId = Int

data Node a = Node
  { nodeId :: NodeId,
    nodeData :: a
  }
  deriving (Show, Eq, Ord)

type Prog = Located Module

data Module = Module
  { moduleName :: Text,
    moduleDecls :: [LBind]
  }
  deriving (Show, Eq, Ord)

type LBind = Located Bind

data Bind
  = BindName Ident ExprId
  | BindFun Ident [Ident] ExprId
  deriving (Show, Eq, Ord)

type ExprId = Int

data Expr
  = ExprLit Lit
  | ExprVar Ident
  | ExprApp ExprId [ExprId]
  | ExprLam [Ident] ExprId
  | ExprLet [LBind] ExprId
  | ExprUnit
  deriving (Show, Eq, Ord)

type Ident = Located (Int, Text)

data Lit
  = LitInt Integer
  | LitBool Bool
  | LitString Text
  deriving (Show, Eq, Ord)
