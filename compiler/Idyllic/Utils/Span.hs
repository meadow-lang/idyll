module Idyllic.Utils.Span (Span (..), toPair, slice, Spanned (..)) where

import Data.ByteString (ByteString)
import qualified Data.ByteString as BS

data Span = Span
  { spanStart :: {-# UNPACK #-} !Int,
    spanEnd :: {-# UNPACK #-} !Int
  }
  deriving (Show, Eq, Ord)

toPair :: Span -> (Int, Int)
toPair (Span s e) = (s, e)

slice :: Span -> ByteString -> ByteString
slice (Span s e) bs = BS.take (e - s) (BS.drop s bs)

-- instance Pretty Span where
--   pretty (Loc s e) = pack $ show s <> ".." <> show e

instance Semigroup Span where
  Span s1 e1 <> Span s2 e2 = Span (min s1 s2) (max e1 e2)

data Spanned a = Spanned {unSpanned :: a, spanOf :: Span}
  deriving (Show, Eq, Ord)

instance Functor Spanned where
  fmap f (Spanned v s) = Spanned (f v) s
