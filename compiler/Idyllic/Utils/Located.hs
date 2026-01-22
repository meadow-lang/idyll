module Idyllic.Utils.Located (Located (..)) where

import Idyllic.Utils.Span (Span)

data Located a = Located {locItem :: a, locSpan :: Span}
  deriving (Show, Eq, Ord)
