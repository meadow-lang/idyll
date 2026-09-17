-- | Port of the @unicode-width@ 0.2.2 crate (UAX #11 display widths), as used
--   by rustyline. The per-code-point data lives in the generated "Tables"
--   module; the string-width state machine below mirrors the crate's
--   @width_in_str@ / @str_width@ exactly, including its emoji, variation
--   selector and ligature special cases.
module System.Console.Rustyline.Internal.Unicode.Width
  ( charWidth,
    strWidth,
  )
where

import Data.Bits (complement, (.&.), (.|.))
import Data.Char (ord)
import Data.Text (Text)
import Data.Text qualified as T
import Data.Word (Word16)
import System.Console.Rustyline.Internal.Unicode.RangeTable
import System.Console.Rustyline.Internal.Unicode.Tables

-- | UAX #11 width of a single character, or 'Nothing' for control characters.
--   Ambiguous-width characters are narrow. (@UnicodeWidthChar::width@.)
charWidth :: Char -> Maybe Int
charWidth c
  | c < '\x7F' = if c >= '\x20' then Just 1 else Nothing
  | c >= '\xA0' = Just (fst (lookupWidth c))
  | otherwise = Nothing

-- | Display width of a string, accounting for emoji sequences, variation
--   selectors and ligatures. (@UnicodeWidthStr::width@.)
strWidth :: Text -> Int
strWidth = fst . T.foldr step (0, wiDefault)
  where
    step c (acc, nextInfo) =
      let (add, info) = widthInStr c nextInfo
       in (acc + add, info)

-- ---------------------------------------------------------------------------
-- WidthInfo

newtype WidthInfo = WidthInfo Word16
  deriving (Eq)

ligatureTransparentMask :: Word16
ligatureTransparentMask = 0x2000

wiDefault, wiLineFeed, wiEmojiModifier, wiRegionalIndicator,
  wiSeveralRegionalIndicator, wiEmojiPresentation, wiZwjEmojiPresentation,
  wiVs16ZwjEmojiPresentation, wiKeycapZwjEmojiPresentation,
  wiVs16KeycapZwjEmojiPresentation, wiRegionalIndicatorZwjPresentation,
  wiEvenRegionalIndicatorZwjPresentation, wiOddRegionalIndicatorZwjPresentation,
  wiTagEndZwjEmojiPresentation, wiTagD1EndZwjEmojiPresentation,
  wiTagD2EndZwjEmojiPresentation, wiTagD3EndZwjEmojiPresentation,
  wiTagA1EndZwjEmojiPresentation, wiTagA2EndZwjEmojiPresentation,
  wiTagA3EndZwjEmojiPresentation, wiTagA4EndZwjEmojiPresentation,
  wiTagA5EndZwjEmojiPresentation, wiTagA6EndZwjEmojiPresentation,
  wiKiratRaiVowelSignE, wiKiratRaiVowelSignAi, wiVariationSelector123,
  wiVariationSelector15, wiVariationSelector16, wiJoiningGroupAlef,
  wiHebrewLetterLamed, wiZwjHebrewLetterLamed, wiBugineseLetterYa,
  wiZwjBugineseLetterYa, wiBugineseVowelSignIZwjLetterYa, wiTifinaghConsonant,
  wiZwjTifinaghConsonant, wiTifinaghJoinerConsonant, wiLisuToneLetterMyaNaJeu,
  wiOldTurkicLetterOrkhonI, wiZwjOldTurkicLetterOrkhonI,
  wiKhmerCoengEligibleLetter ::
    WidthInfo
wiDefault = WidthInfo 0
wiLineFeed = WidthInfo 0b0000000000000001
wiEmojiModifier = WidthInfo 0b0000000000000010
wiRegionalIndicator = WidthInfo 0b0000000000000011
wiSeveralRegionalIndicator = WidthInfo 0b0000000000000100
wiEmojiPresentation = WidthInfo 0b0000000000000101
wiZwjEmojiPresentation = WidthInfo 0b0001000000000110
wiVs16ZwjEmojiPresentation = WidthInfo 0b1001000000000110
wiKeycapZwjEmojiPresentation = WidthInfo 0b0001000000000111
wiVs16KeycapZwjEmojiPresentation = WidthInfo 0b1001000000000111
wiRegionalIndicatorZwjPresentation = WidthInfo 0b0000000000001001
wiEvenRegionalIndicatorZwjPresentation = WidthInfo 0b0000000000001010
wiOddRegionalIndicatorZwjPresentation = WidthInfo 0b0000000000001011
wiTagEndZwjEmojiPresentation = WidthInfo 0b0000000000010000
wiTagD1EndZwjEmojiPresentation = WidthInfo 0b0000000000010001
wiTagD2EndZwjEmojiPresentation = WidthInfo 0b0000000000010010
wiTagD3EndZwjEmojiPresentation = WidthInfo 0b0000000000010011
wiTagA1EndZwjEmojiPresentation = WidthInfo 0b0000000000011001
wiTagA2EndZwjEmojiPresentation = WidthInfo 0b0000000000011010
wiTagA3EndZwjEmojiPresentation = WidthInfo 0b0000000000011011
wiTagA4EndZwjEmojiPresentation = WidthInfo 0b0000000000011100
wiTagA5EndZwjEmojiPresentation = WidthInfo 0b0000000000011101
wiTagA6EndZwjEmojiPresentation = WidthInfo 0b0000000000011110
wiKiratRaiVowelSignE = WidthInfo 0b0000000000100000
wiKiratRaiVowelSignAi = WidthInfo 0b0000000000100001
wiVariationSelector123 = WidthInfo 0b0000001000000000
wiVariationSelector15 = WidthInfo 0b0100000000000000
wiVariationSelector16 = WidthInfo 0b1000000000000000
wiJoiningGroupAlef = WidthInfo 0b0011000011111111
wiHebrewLetterLamed = WidthInfo 0b0011100000000000
wiZwjHebrewLetterLamed = WidthInfo 0b0011110000000000
wiBugineseLetterYa = WidthInfo 0b0011100000000001
wiZwjBugineseLetterYa = WidthInfo 0b0011110000000001
wiBugineseVowelSignIZwjLetterYa = WidthInfo 0b0011110000000010
wiTifinaghConsonant = WidthInfo 0b0011100000000011
wiZwjTifinaghConsonant = WidthInfo 0b0011110000000011
wiTifinaghJoinerConsonant = WidthInfo 0b0011110000000100
wiLisuToneLetterMyaNaJeu = WidthInfo 0b0011110000000101
wiOldTurkicLetterOrkhonI = WidthInfo 0b0011100000000110
wiZwjOldTurkicLetterOrkhonI = WidthInfo 0b0011110000000110
wiKhmerCoengEligibleLetter = WidthInfo 0b0011110000000111

bits :: WidthInfo -> Word16
bits (WidthInfo b) = b

isLigatureTransparentInfo :: WidthInfo -> Bool
isLigatureTransparentInfo (WidthInfo b) = b .&. 0b0000100000000000 == 0b0000100000000000

setZwjBit :: WidthInfo -> WidthInfo
setZwjBit (WidthInfo b) = WidthInfo (b .|. 0b0000010000000000)

isEmojiPresentation :: WidthInfo -> Bool
isEmojiPresentation (WidthInfo b) = b .&. bits wiVariationSelector16 == bits wiVariationSelector16

isZwjEmojiPresentation :: WidthInfo -> Bool
isZwjEmojiPresentation (WidthInfo b) = b .&. 0b1011000000000000 == 0b1001000000000000

hasLigatureMask :: WidthInfo -> Bool
hasLigatureMask (WidthInfo b) = b .&. ligatureTransparentMask == ligatureTransparentMask

setEmojiPresentation :: WidthInfo -> WidthInfo
setEmojiPresentation w@(WidthInfo b)
  | hasLigatureMask w || b .&. 0b1001000000000000 == 0b0001000000000000 =
      -- Rust precedence: `|` binds looser than `&`
      WidthInfo
        ( b
            .|. ( bits wiVariationSelector16
                    .&. complement (bits wiVariationSelector15)
                    .&. complement (bits wiVariationSelector123)
                )
        )
  | otherwise = wiVariationSelector16

unsetEmojiPresentation :: WidthInfo -> WidthInfo
unsetEmojiPresentation w@(WidthInfo b)
  | hasLigatureMask w = WidthInfo (b .&. complement (bits wiVariationSelector16))
  | otherwise = wiDefault

isTextPresentation :: WidthInfo -> Bool
isTextPresentation (WidthInfo b) = b .&. bits wiVariationSelector15 == bits wiVariationSelector15

setTextPresentation :: WidthInfo -> WidthInfo
setTextPresentation w@(WidthInfo b)
  | hasLigatureMask w =
      WidthInfo
        ( b
            .|. ( bits wiVariationSelector15
                    .&. complement (bits wiVariationSelector16)
                    .&. complement (bits wiVariationSelector123)
                )
        )
  | otherwise = wiVariationSelector15

unsetTextPresentation :: WidthInfo -> WidthInfo
unsetTextPresentation (WidthInfo b) = WidthInfo (b .&. complement (bits wiVariationSelector15))

isVs123 :: WidthInfo -> Bool
isVs123 (WidthInfo b) = b .&. bits wiVariationSelector123 == bits wiVariationSelector123

setVs123 :: WidthInfo -> WidthInfo
setVs123 w@(WidthInfo b)
  | hasLigatureMask w =
      WidthInfo
        ( b
            .|. ( bits wiVariationSelector123
                    .&. complement (bits wiVariationSelector15)
                    .&. complement (bits wiVariationSelector16)
                )
        )
  | otherwise = wiVariationSelector123

unsetVs123 :: WidthInfo -> WidthInfo
unsetVs123 (WidthInfo b) = WidthInfo (b .&. complement (bits wiVariationSelector123))

-- ---------------------------------------------------------------------------
-- Lookups

between :: Char -> Char -> Char -> Bool
between lo hi c = c >= lo && c <= hi

lookupWidth :: Char -> (Int, WidthInfo)
lookupWidth c
  | w < 3 = (w, wiDefault)
  | c == '\x0A' = (1, wiLineFeed)
  | c == '\x5DC' = (1, wiHebrewLetterLamed)
  | between '\x622' '\x882' c = (1, wiJoiningGroupAlef)
  | between '\x1780' '\x17AF' c = (1, wiKhmerCoengEligibleLetter)
  | c == '\x17D8' = (3, wiDefault)
  | c == '\x1A10' = (1, wiBugineseLetterYa)
  | between '\x2D31' '\x2D6F' c = (1, wiTifinaghConsonant)
  | between '\xA4FC' '\xA4FD' c = (1, wiLisuToneLetterMyaNaJeu)
  | c == '\xFE01' = (0, wiVariationSelector123)
  | c == '\xFE0E' = (0, wiVariationSelector15)
  | c == '\xFE0F' = (0, wiVariationSelector16)
  | c == '\x10C03' = (1, wiOldTurkicLetterOrkhonI)
  | c == '\x16D67' = (1, wiKiratRaiVowelSignE)
  | c == '\x16D68' = (1, wiKiratRaiVowelSignAi)
  | between '\x1F1E6' '\x1F1FF' c = (1, wiRegionalIndicator)
  | between '\x1F3FB' '\x1F3FF' c = (2, wiEmojiModifier)
  | otherwise = (2, wiEmojiPresentation)
  where
    w = lookupRange widthTable 1 (ord c)

isTransparentZeroWidth :: Char -> Bool
isTransparentZeroWidth c =
  fst (lookupWidth c) == 0 && not (inRange nonTransparentZeroWidthTable (ord c))

isLigatureTransparent :: Char -> Bool
isLigatureTransparent c =
  c == '\x34F'
    || between '\x17B4' '\x17B5' c
    || between '\x180B' '\x180D' c
    || c == '\x180F'
    || c == '\x200D'
    || between '\xFE00' '\xFE0F' c
    || between '\xE0100' '\xE01EF' c

startsEmojiPresentationSeq :: Char -> Bool
startsEmojiPresentationSeq c = inRange emojiPresentationTable (ord c)

startsNonIdeographicTextPresentationSeq :: Char -> Bool
startsNonIdeographicTextPresentationSeq c = inRange textPresentationTable (ord c)

isEmojiModifierBase :: Char -> Bool
isEmojiModifierBase c = inRange emojiModifierBaseTable (ord c)

isRegionalIndicator :: Char -> Bool
isRegionalIndicator = between '\x1F1E6' '\x1F1FF'

isTagLetter :: Char -> Bool
isTagLetter = between '\xE0061' '\xE007A'

isTagDigit :: Char -> Bool
isTagDigit = between '\xE0030' '\xE0039'

isTifinaghBase :: Char -> Bool
isTifinaghBase c = between '\x2D31' '\x2D65' c || c == '\x2D6F'

-- | Width contribution of @c@ given what follows it, and the info to pass to
--   the character before it. (@width_in_str@.)
widthInStr :: Char -> WidthInfo -> (Int, WidthInfo)
widthInStr c nextInfo0
  | isEmojiPresentation nextInfo0 && startsEmojiPresentationSeq c =
      (if isZwjEmojiPresentation nextInfo0 then 0 else 2, wiEmojiPresentation)
  | otherwise =
      let nextInfo1 =
            if isEmojiPresentation nextInfo0
              then unsetEmojiPresentation nextInfo0
              else nextInfo0
       in if c <= '\xA0'
            then case c of
              '\n' -> (1, wiLineFeed)
              '\r' | nextInfo1 == wiLineFeed -> (0, wiDefault)
              _ -> (1, wiDefault)
            else
              if nextInfo1 /= wiDefault
                then special nextInfo1
                else fallback
  where
    fallback = lookupWidth c

    special ni
      | c == '\xFE0F' = (0, setEmojiPresentation ni)
      | c == '\xFE01' = (0, setVs123 ni)
      | c == '\xFE0E' = (0, setTextPresentation ni)
      | isTextPresentation ni =
          if startsNonIdeographicTextPresentationSeq c
            then (1, wiDefault)
            else afterPresentation (unsetTextPresentation ni)
      | isVs123 ni =
          if c `elem` ['\x2018', '\x2019', '\x201C', '\x201D']
            then (2, wiDefault)
            else afterPresentation (unsetVs123 ni)
      | otherwise = afterPresentation ni

    afterPresentation ni
      | isLigatureTransparentInfo ni && c == '\x200D' = (0, setZwjBit ni)
      | isLigatureTransparentInfo ni && isLigatureTransparent c = (0, ni)
      | otherwise = pairs ni

    pairs ni
      -- Arabic Lam-Alef ligature
      | ni == wiJoiningGroupAlef
          && ( c == '\x644'
                 || between '\x6B5' '\x6B8' c
                 || c == '\x76A'
                 || c == '\x8A6'
                 || c == '\x8C7'
             ) =
          (0, wiDefault)
      | ni == wiJoiningGroupAlef && isTransparentZeroWidth c = (0, wiJoiningGroupAlef)
      -- Hebrew Alef-ZWJ-Lamed ligature
      | ni == wiZwjHebrewLetterLamed && c == '\x05D0' = (0, wiDefault)
      -- Khmer coeng signs
      | ni == wiKhmerCoengEligibleLetter && c == '\x17D2' = (-1, wiDefault)
      -- Buginese <a, -i> ZWJ ya ligature
      | ni == wiZwjBugineseLetterYa && c == '\x1A17' = (0, wiBugineseVowelSignIZwjLetterYa)
      | ni == wiBugineseVowelSignIZwjLetterYa && c == '\x1A15' = (0, wiDefault)
      -- Tifinagh bi-consonants
      | (ni == wiTifinaghConsonant || ni == wiZwjTifinaghConsonant) && c == '\x2D7F' =
          (1, wiTifinaghJoinerConsonant)
      | ni == wiZwjTifinaghConsonant && isTifinaghBase c = (0, wiDefault)
      | ni == wiTifinaghJoinerConsonant && isTifinaghBase c = (-1, wiDefault)
      -- Lisu tone letter combinations
      | ni == wiLisuToneLetterMyaNaJeu && between '\xA4F8' '\xA4FB' c = (0, wiDefault)
      -- Old Turkic ligature
      | ni == wiZwjOldTurkicLetterOrkhonI && c == '\x10C32' = (0, wiDefault)
      -- Emoji modifier
      | ni == wiEmojiModifier && isEmojiModifierBase c = (0, wiEmojiPresentation)
      -- Regional indicator
      | (ni == wiRegionalIndicator || ni == wiSeveralRegionalIndicator) && isRegionalIndicator c =
          (1, wiSeveralRegionalIndicator)
      -- ZWJ emoji
      | ni
          `elem` [ wiEmojiPresentation,
                   wiSeveralRegionalIndicator,
                   wiEvenRegionalIndicatorZwjPresentation,
                   wiOddRegionalIndicatorZwjPresentation,
                   wiEmojiModifier
                 ]
          && c == '\x200D' =
          (0, wiZwjEmojiPresentation)
      | ni == wiZwjEmojiPresentation && c == '\x20E3' = (0, wiKeycapZwjEmojiPresentation)
      | ni == wiVs16ZwjEmojiPresentation && startsEmojiPresentationSeq c = (0, wiEmojiPresentation)
      | ni == wiVs16KeycapZwjEmojiPresentation && (between '0' '9' c || c == '#' || c == '*') =
          (0, wiEmojiPresentation)
      | ni == wiZwjEmojiPresentation && isRegionalIndicator c = (1, wiRegionalIndicatorZwjPresentation)
      | (ni == wiRegionalIndicatorZwjPresentation || ni == wiOddRegionalIndicatorZwjPresentation)
          && isRegionalIndicator c =
          (-1, wiEvenRegionalIndicatorZwjPresentation)
      | ni == wiEvenRegionalIndicatorZwjPresentation && isRegionalIndicator c =
          (3, wiOddRegionalIndicatorZwjPresentation)
      | ni == wiZwjEmojiPresentation && between '\x1F3FB' '\x1F3FF' c = (0, wiEmojiModifier)
      | ni == wiZwjEmojiPresentation && c == '\xE007F' = (0, wiTagEndZwjEmojiPresentation)
      | ni == wiTagEndZwjEmojiPresentation && isTagLetter c = (0, wiTagA1EndZwjEmojiPresentation)
      | ni == wiTagA1EndZwjEmojiPresentation && isTagLetter c = (0, wiTagA2EndZwjEmojiPresentation)
      | ni == wiTagA2EndZwjEmojiPresentation && isTagLetter c = (0, wiTagA3EndZwjEmojiPresentation)
      | ni == wiTagA3EndZwjEmojiPresentation && isTagLetter c = (0, wiTagA4EndZwjEmojiPresentation)
      | ni == wiTagA4EndZwjEmojiPresentation && isTagLetter c = (0, wiTagA5EndZwjEmojiPresentation)
      | ni == wiTagA5EndZwjEmojiPresentation && isTagLetter c = (0, wiTagA6EndZwjEmojiPresentation)
      | ni
          `elem` [ wiTagEndZwjEmojiPresentation,
                   wiTagA1EndZwjEmojiPresentation,
                   wiTagA2EndZwjEmojiPresentation,
                   wiTagA3EndZwjEmojiPresentation,
                   wiTagA4EndZwjEmojiPresentation
                 ]
          && isTagDigit c =
          (0, wiTagD1EndZwjEmojiPresentation)
      | ni == wiTagD1EndZwjEmojiPresentation && isTagDigit c = (0, wiTagD2EndZwjEmojiPresentation)
      | ni == wiTagD2EndZwjEmojiPresentation && isTagDigit c = (0, wiTagD3EndZwjEmojiPresentation)
      | ni
          `elem` [ wiTagA3EndZwjEmojiPresentation,
                   wiTagA4EndZwjEmojiPresentation,
                   wiTagA5EndZwjEmojiPresentation,
                   wiTagA6EndZwjEmojiPresentation,
                   wiTagD3EndZwjEmojiPresentation
                 ]
          && c == '\x1F3F4' =
          (0, wiEmojiPresentation)
      | ni == wiZwjEmojiPresentation && snd (lookupWidth c) == wiEmojiPresentation =
          (0, wiEmojiPresentation)
      | ni == wiKiratRaiVowelSignE && c == '\x16D63' = (0, wiDefault)
      | ni == wiKiratRaiVowelSignE && c == '\x16D67' = (0, wiKiratRaiVowelSignAi)
      | ni == wiKiratRaiVowelSignE && c == '\x16D68' = (1, wiKiratRaiVowelSignE)
      | ni == wiKiratRaiVowelSignE && c == '\x16D69' = (0, wiDefault)
      | ni == wiKiratRaiVowelSignAi && c == '\x16D63' = (0, wiDefault)
      | otherwise = fallback
