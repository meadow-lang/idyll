module Idyllic.HIR
  ( Prog,
    Module (..),
    LBind,
    Bind (..),
    LExpr,
    Expr (..),
    Ident,
    Lit (..),
  )
where

import Data.Text (Text)
import Idyllic.Utils (Located)

type Prog = Located Module

data Module = Module
  { moduleName :: Text,
    moduleDecls :: [LBind]
  }
  deriving (Show, Eq, Ord)

type LBind = Located Bind

data Bind
  = BindName Ident LExpr
  | BindFun Ident [Ident] LExpr
  deriving (Show, Eq, Ord)

type LExpr = Located Expr

data Expr
  = ExprLit Lit
  | ExprVar Ident
  | ExprApp LExpr [LExpr]
  | ExprLam [Ident] LExpr
  | ExprLet [LBind] LExpr
  | ExprUnit
  deriving (Show, Eq, Ord)

type Ident = Located Int

data Lit
  = Int Integer
  | Bool Bool
  | String Text
  deriving (Show, Eq, Ord)
