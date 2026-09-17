{-# LANGUAGE CPP #-}

-- | Completion API (@completion.rs@).
module System.Console.Rustyline.Completion
  ( Pair (..),
    candidate,
    Completer (..),
    defaultCompleter,
    replaceCompletion,

    -- * Filenames
    Quote (..),
    filenameCompleter,
    completePath,
    completePathUnsorted,
    defaultBreakChars,
    doubleQuotesSpecialChars,
    unescape,
    escape,
    extractWord,
    longestCommonPrefix,
    findUnclosedQuote,
  )
where

import Control.Exception (IOException, try)
import Control.Monad.State.Strict (gets)
import Data.List (sortOn)
import Data.Maybe (catMaybes)
import Data.Text (Text)
import Data.Text qualified as T
import System.Console.Rustyline.Context (Context)
import System.Console.Rustyline.Internal.Text
import System.Console.Rustyline.LineBuffer (LB, pos, replace)
import System.Console.Rustyline.Undo (Changeset)
import System.Directory (doesPathExist, getCurrentDirectory, getHomeDirectory, listDirectory)
import System.FilePath (isRelative, splitDirectories, (</>))
import System.Posix.Files (getFileStatus, isDirectory)

-- | A completion candidate.
data Pair = Pair
  { -- | Text to display when listing alternatives.
    pairDisplay :: Text,
    -- | Text to insert in the line.
    pairReplacement :: Text
  }
  deriving (Eq, Show)

-- | A candidate that displays and inserts the same text.
candidate :: Text -> Pair
candidate t = Pair t t

-- | Tab completion.
data Completer = Completer
  { -- | Takes the currently edited line with the cursor position and returns
    --   the start position and the candidates for the partial word to be
    --   completed: @("ls /usr/loc", 11)@ gives @(3, ["/usr/local/"])@.
    complete :: Text -> Int -> Context -> IO (Int, [Pair]),
    -- | Updates the edited line with the elected candidate (given the start
    --   position).
    updateCompletion :: Int -> Text -> LB Changeset ()
  }

-- | No completion.
defaultCompleter :: Completer
defaultCompleter = Completer (\_ _ _ -> pure (0, [])) replaceCompletion

-- | Replace the text between @start@ and the cursor with @elected@.
replaceCompletion :: Int -> Text -> LB Changeset ()
replaceCompletion start elected = do
  end <- gets (pos . fst)
  replace start end elected

-- | Kind of quote.
data Quote
  = -- | Double quote: @"@
    Double
  | -- | Single quote: @'@
    Single
  | -- | No quote
    NoQuote
  deriving (Eq, Show)

-- | rl_basic_word_break_characters, rl_completer_word_break_characters
defaultBreakChars :: Char -> Bool
defaultBreakChars c = c `elem` (" \t\n\"\\'`@$><=;|&{(\0" :: String)

escapeChar :: Maybe Char
escapeChar = Just '\\'

doubleQuotesEscapeChar :: Maybe Char
doubleQuotesEscapeChar = Just '\\'

-- | In double quotes, not all break chars need to be escaped.
doubleQuotesSpecialChars :: Char -> Bool
doubleQuotesSpecialChars c = c `elem` ("\"$\\`" :: String)

-- | A completer for file and folder names.
filenameCompleter :: Completer
filenameCompleter = defaultCompleter {complete = \line p _ -> completePath line p}

-- | Candidates for the partial path before the cursor, sorted.
completePath :: Text -> Int -> IO (Int, [Pair])
completePath line p = do
  (start, matches) <- completePathUnsorted line p
  pure (start, sortOn pairDisplay matches)

-- | Like 'completePath', but unsorted.
completePathUnsorted :: Text -> Int -> IO (Int, [Pair])
completePathUnsorted line p = do
  let beforeCursor = takeB p line
      (start, path, escChar, breakChars, quote) = case findUnclosedQuote beforeCursor of
        Just (idx, q) ->
          let s = idx + 1
           in if q == Double
                then (s, unescape (sliceB s p line) doubleQuotesEscapeChar, doubleQuotesEscapeChar, doubleQuotesSpecialChars, q)
                else (s, sliceB s p line, Nothing, defaultBreakChars, q)
        Nothing ->
          let (s, word) = extractWord line p escapeChar defaultBreakChars
           in (s, unescape word escapeChar, escapeChar, defaultBreakChars, NoQuote)
  matches <- filenameComplete path escChar breakChars quote
  pure (start, matches)

-- | Remove the escape char.
unescape :: Text -> Maybe Char -> Text
unescape input Nothing = input
unescape input (Just esc)
  | not (T.any (== esc) input) = input
  | otherwise = T.pack (go (T.unpack input))
  where
    go (c : rest)
      | c == esc = case rest of
          (c' : rest') -> c' : go rest'
          [] -> []
      | otherwise = c : go rest
    go [] = []

-- | Escape any break chars in the input with the escape char:
--   @/User Information@ becomes @/User\\ Information@.
escape :: Text -> Maybe Char -> (Char -> Bool) -> Quote -> Text
escape input escChar isBreakChar quote
  | quote == Single = input -- no escape in single quotes
  | not (T.any isBreakChar input) = input -- no need to escape
  | otherwise = case escChar of
      Nothing -> input
      Just esc -> T.concatMap (\c -> if isBreakChar c then T.pack [esc, c] else T.singleton c) input

filenameComplete :: Text -> Maybe Char -> (Char -> Bool) -> Quote -> IO [Pair]
filenameComplete path escChar isBreakChar quote = do
  let (dirName, fileName) = case rfindB '/' path of
        Just idx -> splitAtB (idx + 1) path
        Nothing -> ("", path)
      dirPath = T.unpack dirName
  dir <- case splitDirectories dirPath of
    ("~" : rest) -> do
      home <- try @IOException getHomeDirectory
      pure $ either (const dirPath) (\h -> foldl (</>) h rest) home
    _
      | isRelative dirPath -> do
          cwd <- try @IOException getCurrentDirectory
          pure $ either (const dirPath) (</> dirPath) cwd
      | otherwise -> pure dirPath
  -- if dir doesn't exist, then don't offer any completions
  exists <- doesPathExist dir
  if not exists
    then pure []
    else do
      -- if any of the below IO operations have errors, just ignore them
      names <- either (const []) id <$> try @IOException (listDirectory dir)
      let prefix = normalize fileName
      fmap catMaybes . mapM (entry dir dirName prefix) $ filter validName names
  where
    validName = all (\c -> c < '\xDC80' || c > '\xDCFF')
    entry dir dirName prefix name
      | not (prefix `T.isPrefixOf` normalize s) = pure Nothing
      | otherwise = do
          status <- try @IOException (getFileStatus (dir </> name))
          pure $ case status of
            Left _ -> Nothing -- ignore PermissionDenied
            Right st ->
              let p = dirName <> s <> (if isDirectory st then "/" else "")
               in Just (Pair s (escape p escChar isBreakChar quote))
      where
        s = T.pack name

-- | Case-insensitive on macOS, as in rustyline.
normalize :: Text -> Text
#if darwin_HOST_OS
normalize = T.toLower
#else
normalize = id
#endif

-- | Given a line and a cursor position, find backward the start of a word.
--   Returns @(0, line[..pos])@ if no break char has been found, the word and
--   its start position otherwise.
extractWord :: Text -> Int -> Maybe Char -> (Char -> Bool) -> (Int, Text)
extractWord line0 p escChar isBreakChar
  | T.null line = (0, line)
  | otherwise = case go Nothing (reverse (charIndices line)) of
      Just start -> (start, dropB start line)
      Nothing -> (0, line)
  where
    line = takeB p line0
    go start [] = start
    go start ((i, c) : rest)
      | Just esc <- escChar,
        Just _ <- start =
          if esc == c
            then go Nothing rest -- escaped break char
            else start
      | isBreakChar c =
          let start' = Just (i + charLenUtf8 c)
           in case escChar of
                Nothing -> start'
                Just _ -> go start' rest -- maybe escaped...
      | otherwise = go start rest

-- | The longest common prefix of all the candidates' replacements.
longestCommonPrefix :: [Pair] -> Maybe Text
longestCommonPrefix [] = Nothing
longestCommonPrefix [c] = Just (pairReplacement c)
longestCommonPrefix candidates@(c0 : _) =
  let reps = map pairReplacement candidates
      lens = map lenB reps
      agrees n = all (> n) lens && allSame (map (byteAt n) reps)
      lcp = length (takeWhile agrees [0 ..])
      first = pairReplacement c0
      bounded = floorCharBoundary lcp first
   in if bounded == 0 then Nothing else Just (takeB bounded first)
  where
    allSame (x : xs) = all (== x) xs
    allSame [] = True
    byteAt n t = takeB 1 (dropB n t) -- only compared, never decoded

data ScanMode
  = SMDoubleQuote
  | SMEscape
  | SMEscapeInDoubleQuote
  | SMNormal
  | SMSingleQuote
  deriving (Eq)

-- | Find an unclosed single/double quote: its byte position and kind.
findUnclosedQuote :: Text -> Maybe (Int, Quote)
findUnclosedQuote s = case foldl step (SMNormal, 0) (charIndices s) of
  (SMDoubleQuote, qi) -> Just (qi, Double)
  (SMEscapeInDoubleQuote, qi) -> Just (qi, Double)
  (SMSingleQuote, qi) -> Just (qi, Single)
  _ -> Nothing
  where
    step (mode, qi) (index, c) = case mode of
      SMDoubleQuote
        | c == '"' -> (SMNormal, qi)
        | c == '\\' -> (SMEscapeInDoubleQuote, qi) -- escape in double quote
        | otherwise -> (mode, qi)
      SMEscape -> (SMNormal, qi)
      SMEscapeInDoubleQuote -> (SMDoubleQuote, qi)
      SMNormal
        | c == '"' -> (SMDoubleQuote, index)
        | c == '\\' -> (SMEscape, qi)
        | c == '\'' -> (SMSingleQuote, index)
        | otherwise -> (mode, qi)
      SMSingleQuote
        | c == '\'' -> (SMNormal, qi) -- no escape in single quotes
        | otherwise -> (mode, qi)
