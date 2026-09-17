-- | Hints: suggestions at the right of the prompt as you type (@hint.rs@).
module System.Console.Rustyline.Hint
  ( Hint (..),
    hintText,
    Hinter,
    noHinter,
    historyHinter,
  )
where

import Data.Text (Text)
import System.Console.Rustyline.Context
import System.Console.Rustyline.History
import System.Console.Rustyline.Internal.Text (dropB, lenB)

-- | A hint returned by a 'Hinter'.
data Hint = Hint
  { -- | Text to display when the hint is active
    hintDisplay :: Text,
    -- | Text to insert in the line when the right arrow is pressed
    hintCompletion :: Maybe Text
  }
  deriving (Eq, Show)

-- | A hint that displays and inserts the same text.
hintText :: Text -> Hint
hintText t = Hint t (Just t)

-- | Takes the currently edited line with the cursor position and returns the
--   hint to display, if any.
type Hinter = Text -> Int -> Context -> IO (Maybe Hint)

noHinter :: Hinter
noHinter _ _ _ = pure Nothing

-- | Suggest the rest of the previous history entry matching the input.
historyHinter :: Hinter
historyHinter line p ctx
  | line == "" || p < lenB line = pure Nothing
  | otherwise = do
      let history = ctxHistory ctx
          index = ctxHistoryIndex ctx
      len <- historyLen history
      let start = if index == len then max 0 (index - 1) else index
      r <- historyStartsWith history line start SearchReverse
      pure $ case r of
        Just sr
          | srEntry sr == line -> Nothing
          | otherwise -> Just (hintText (dropB p (srEntry sr)))
        Nothing -> Nothing
