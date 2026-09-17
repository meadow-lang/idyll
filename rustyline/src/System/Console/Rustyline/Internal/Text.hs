-- | 'Text' helpers that work in UTF-8 byte offsets, matching the byte
--   positions rustyline uses throughout (text >= 2.0 stores UTF-8, so these
--   are O(1) slices). Every offset passed in must lie on a character
--   boundary, exactly as Rust's @str@ slicing requires.
module System.Console.Rustyline.Internal.Text
  ( lenB,
    takeB,
    dropB,
    sliceB,
    splitAtB,
    findB,
    rfindB,
    findSubB,
    charLenUtf8,
    charIndices,
    lastChar,
    floorCharBoundary,
    trimStartRust,
    trimEndRust,
  )
where

import Data.Text (Text)
import Data.Text qualified as T
import Data.Text.Unsafe qualified as TU
import System.Console.Rustyline.Internal.Unicode.CharProps (isWhitespace)

-- | Length in UTF-8 bytes (@str::len@).
lenB :: Text -> Int
lenB = TU.lengthWord8

takeB :: Int -> Text -> Text
takeB = TU.takeWord8

dropB :: Int -> Text -> Text
dropB = TU.dropWord8

-- | @&s[start..end]@
sliceB :: Int -> Int -> Text -> Text
sliceB start end t = takeB (end - start) (dropB start t)

splitAtB :: Int -> Text -> (Text, Text)
splitAtB n t = (takeB n t, dropB n t)

-- | Byte offset of the first occurrence of the character (@str::find@).
findB :: Char -> Text -> Maybe Int
findB c t = case T.break (== c) t of
  (pre, rest)
    | T.null rest -> Nothing
    | otherwise -> Just (lenB pre)

-- | Byte offset of the last occurrence of the character (@str::rfind@).
rfindB :: Char -> Text -> Maybe Int
rfindB c t = case T.breakOnEnd (T.singleton c) t of
  (pre, _)
    | T.null pre -> Nothing
    | otherwise -> Just (lenB pre - charLenUtf8 c)

-- | Byte offset of the first occurrence of a substring (@str::find(&str)@).
findSubB :: Text -> Text -> Maybe Int
findSubB needle hay
  | T.null needle = Just 0
  | otherwise = case T.breakOn needle hay of
      (pre, rest)
        | T.null rest -> Nothing
        | otherwise -> Just (lenB pre)

-- | @char::len_utf8@
charLenUtf8 :: Char -> Int
charLenUtf8 c
  | n < 0x80 = 1
  | n < 0x800 = 2
  | n < 0x10000 = 3
  | otherwise = 4
  where
    n = fromEnum c

-- | Characters with their UTF-8 byte offsets (@str::char_indices@).
charIndices :: Text -> [(Int, Char)]
charIndices = go 0 . T.unpack
  where
    go _ [] = []
    go i (c : cs) = (i, c) : go (i + charLenUtf8 c) cs

lastChar :: Text -> Maybe Char
lastChar t
  | T.null t = Nothing
  | otherwise = Just (T.last t)

-- | Largest character boundary not after the byte offset.
floorCharBoundary :: Int -> Text -> Int
floorCharBoundary n t
  | n >= lenB t = lenB t
  | otherwise = go 0 (T.unpack t)
  where
    go i (c : cs)
      | i + charLenUtf8 c > n = i
      | otherwise = go (i + charLenUtf8 c) cs
    go i [] = i

-- | @str::trim_start@ (Rust's notion of whitespace).
trimStartRust :: Text -> Text
trimStartRust = T.dropWhile isWhitespace

-- | @str::trim_end@ (Rust's notion of whitespace).
trimEndRust :: Text -> Text
trimEndRust = T.dropWhileEnd isWhitespace
