module Idyllic.Types where

import Data.Map (Map)
import Data.Text (Text)

newtype RenameError = ResUnboundName Text deriving (Show, Eq)

data NameEnv = NameEnv
  { nameMap :: Map Text Int,
    nameCounter :: Int,
    nodeIdCounter :: Int,
    errors :: [RenameError]
  }
  deriving (Show, Eq)