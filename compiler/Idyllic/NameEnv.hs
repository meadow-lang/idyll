module Idyllic.RenameEnv where

import Data.Map (Map)
import Data.Text (Text)
import Idyllic.Schema

data RenameEnv = NameEnv
  { nameMap :: Map Text Int,
    nameCounter :: Int,
    nodeIdCounter :: Int,
    errors :: [RenameError]
  }
  deriving (Show, Eq)