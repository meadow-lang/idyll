module Idyllic.Layout where

import Data.Text
import Idyllic.Token

data Delim
  = Parens
  | Braces
  | Brackets
  | Layout Herald
  deriving (Show, Eq, Ord)

data Herald = Let | Of | Where | Do deriving (Show, Eq, Ord)

data LayoutContext = LayoutContext
  { indent :: Int,
    delim :: Delim
  }
  deriving (Show)

type LayoutStack = [LayoutContext]

layDKillaz :: [(Herald, Text)]
layDKillaz = [(Of, "where"), (Do, "where"), (Let, "in")]

closingDelim :: [Text]
closingDelim = [")", "}", "]", ";"]

layout :: [LToken] -> LayoutStack -> [LToken]
layout = undefined
