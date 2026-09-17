-- | Syntax highlighting (@highlight.rs@).
module System.Console.Rustyline.Highlight
  ( CmdKind (..),
    Highlighter (..),
    defaultHighlighter,
    newMatchingBracketHighlighter,

    -- * Internals (exposed for tests)
    findMatchingBracket,
    checkBracket,
    matchingBracket,
    isOpenBracket,
    isCloseBracket,
  )
where

import Data.ByteString qualified as BS
import Data.ByteString.Char8 qualified as BC
import Data.IORef
import Data.Text (Text)
import Data.Text qualified as T
import Data.Text.Encoding qualified as TE
import System.Console.Rustyline.Config (CompletionType)
import System.Console.Rustyline.Internal.Text (dropB, takeB)

-- | Which kind of action triggered the call to the 'Highlighter'.
data CmdKind
  = -- | Cursor moved
    MoveCursor
  | -- | Other action
    OtherKind
  | -- | Forced / final refresh (no auto-suggestion / hint, no matching
    --   bracket highlighted, ...)
    ForcedRefresh
  deriving (Eq, Show)

-- | Syntax highlighter with ANSI colours. The highlighted version must have
--   the same display width as the original input.
data Highlighter = Highlighter
  { -- | Highlight the currently edited line (cursor at the given byte
    --   position).
    highlight :: Text -> Int -> IO Text,
    -- | Highlight the prompt; the flag says whether it is the default prompt.
    highlightPrompt :: Text -> Bool -> IO Text,
    -- | Highlight a hint.
    highlightHint :: Text -> IO Text,
    -- | Highlight a completion candidate (only used with 'List').
    highlightCandidate :: Text -> CompletionType -> IO Text,
    -- | Does the line need to be highlighted when a specific char is typed
    --   or when the cursor is moved under a specific char? Used to optimize
    --   refresh.
    highlightChar :: Text -> Int -> CmdKind -> IO Bool
  }

-- | No highlighting.
defaultHighlighter :: Highlighter
defaultHighlighter =
  Highlighter
    { highlight = \line _ -> pure line,
      highlightPrompt = \p _ -> pure p,
      highlightHint = pure,
      highlightCandidate = \c _ -> pure c,
      highlightChar = \_ _ _ -> pure False
    }

-- | Highlight the matching bracket when one is typed or the cursor moves
--   onto one.
newMatchingBracketHighlighter :: IO Highlighter
newMatchingBracketHighlighter = do
  bracketRef <- newIORef Nothing -- memorize the character to search...
  pure
    defaultHighlighter
      { highlight = \line _ ->
          if BS.length (TE.encodeUtf8 line) <= 1
            then pure line
            else do
              bracket <- readIORef bracketRef
              pure $ case bracket >>= \(b, p) -> findMatchingBracket line p b of
                Just (matching, idx) ->
                  takeB idx line
                    <> "\x1b[1;34m"
                    <> T.singleton matching
                    <> "\x1b[0m"
                    <> dropB (idx + 1) line
                Nothing -> line,
        highlightChar = \line p kind ->
          if kind == ForcedRefresh
            then False <$ writeIORef bracketRef Nothing
            else do
              let b = checkBracket line p
              writeIORef bracketRef b
              pure (b /= Nothing)
      }

-- | Find the bracket matching the one at byte @pos@.
findMatchingBracket :: Text -> Int -> Char -> Maybe (Char, Int)
findMatchingBracket line p bracket
  | isOpenBracket bracket = forward (1 :: Int) (p + 1)
  | otherwise = backward (1 :: Int) p
  where
    bytes = TE.encodeUtf8 line
    matching = matchingBracket bracket
    forward unmatched idx
      | idx >= BS.length bytes = Nothing
      | b == matching = if unmatched == 1 then Just (matching, idx) else forward (unmatched - 1) (idx + 1)
      | b == bracket = forward (unmatched + 1) (idx + 1)
      | otherwise = forward unmatched (idx + 1)
      where
        b = BC.index bytes idx
    backward unmatched idx
      | idx <= 0 = Nothing
      | b == matching = if unmatched == 1 then Just (matching, idx - 1) else backward (unmatched - 1) (idx - 1)
      | b == bracket = backward (unmatched + 1) (idx - 1)
      | otherwise = backward unmatched (idx - 1)
      where
        b = BC.index bytes (idx - 1)

-- | Check under or before the cursor.
checkBracket :: Text -> Int -> Maybe (Char, Int)
checkBracket line p0
  | BS.null bytes = Nothing
  | p0 >= len =
      let p = len - 1 -- before cursor
          b = BC.index bytes p -- previous byte
       in if isCloseBracket b then Just (b, p) else Nothing
  | otherwise = go True p0
  where
    bytes = TE.encodeUtf8 line
    len = BS.length bytes
    go underCursor p
      | isCloseBracket b = if p == 0 then Nothing else Just (b, p)
      | isOpenBracket b = if p + 1 == len then Nothing else Just (b, p)
      | underCursor && p > 0 = go False (p - 1) -- or before cursor
      | otherwise = Nothing
      where
        b = BC.index bytes p

matchingBracket :: Char -> Char
matchingBracket = \case
  '{' -> '}'
  '}' -> '{'
  '[' -> ']'
  ']' -> '['
  '(' -> ')'
  ')' -> '('
  b -> b

isOpenBracket :: Char -> Bool
isOpenBracket b = b == '{' || b == '[' || b == '('

isCloseBracket :: Char -> Bool
isCloseBracket b = b == '}' || b == ']' || b == ')'
