module Idyllic.Core where

import Data.Text (Text)

data Expr = Expr
  { exprKind :: ExprKind,
    exprType :: Ty
  }
  deriving (Show, Eq, Ord)

data ExprKind
  = ExprInt Int
  | ExprVar Text
  | ExprLet Text Expr Expr
  | ExprLetRec Text [(Text, Expr)] Expr
  | ExprLam Text Expr
  | ExprApp Expr Expr
  deriving (Show, Eq, Ord)

data Ty = TyApp Text [Ty] deriving (Show, Eq, Ord)