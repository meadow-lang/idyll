-- | Rust @char@ predicates that rustyline relies on and whose GHC
--   counterparts ('Data.Char.isAlphaNum', 'Data.Char.isSpace') disagree on a
--   handful of code points.
module System.Console.Rustyline.Internal.Unicode.CharProps
  ( isAlphanumeric,
    isWhitespace,
  )
where

import Data.Char (ord)
import System.Console.Rustyline.Internal.Unicode.RangeTable
import System.Console.Rustyline.Internal.Unicode.Tables

-- | Rust's @char::is_alphanumeric@ (Alphabetic or Numeric).
isAlphanumeric :: Char -> Bool
isAlphanumeric c = inRange alphanumericTable (ord c)

-- | Rust's @char::is_whitespace@ (the White_Space property).
isWhitespace :: Char -> Bool
isWhitespace c = inRange whitespaceTable (ord c)
