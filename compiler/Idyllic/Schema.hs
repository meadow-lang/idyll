module Idyllic.Schema where

import Data.Map (Map)
import Data.Text (Text)

newtype RenameError = ResUnboundName Text deriving (Show, Eq)

