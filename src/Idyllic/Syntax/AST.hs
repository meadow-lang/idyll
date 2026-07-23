module Idyllic.Syntax.AST where

import Data.Text (Text)
import Idyllic.Utils (Located)

type Prog = LTerm

type LTerm = Located Term

data Term
  = TermLit Lit
  | TermVar Ident
  | TermLet [LBind] LTerm
  | TermLam [Ident] LTerm
  | TermApp LTerm [LTerm]
  | TermNeg LTerm
  | TermList [LTerm]
  | TermPi (Maybe Ident) LTerm LTerm -- (x : a) -> b, (?x : a) -> b, a -> b
  | TermAnn LTerm LTerm -- e : a  (type annotation)
  | TermType
  deriving (Show, Eq, Ord)

type LBind = Located Bind

data Bind
  = BindName Ident LTerm
  | BindFun Ident [Ident] LTerm
  deriving (Show, Eq, Ord)

type Ident = Located Text

data Lit
  = LitUnit
  | LitInt Integer
  | LitBool Bool
  | LitString Text
  deriving (Show, Eq, Ord)
