module Idyllic.Rename.Env where

import Data.Map (Map)
import Data.Text (Text)
import Idyllic.Schema

data RenameEnv = RenameEnv
  { nameMap :: Map Text Int,
    nameCounter :: Int,
    nodeIdCounter :: Int,
    errors :: [Text]
  }
  deriving (Show, Eq)