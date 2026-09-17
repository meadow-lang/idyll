-- | Readline for Haskell: a port of the Rust @rustyline@ crate (18.0.1).
--
-- @
-- import System.Console.Rustyline
--
-- main :: IO ()
-- main = do
--   rl <- newEditor defaultConfig
--   r <- readline rl ">> "
--   case r of
--     Right line -> putStrLn ("Line: " <> show line)
--     Left _ -> putStrLn "No input"
-- @
--
-- Rust's traits are records of functions here ('Helper', 'Completer',
-- 'Highlighter', 'Validator', 'Terminal', ...) and the editor is a mutable
-- handle.
module System.Console.Rustyline
  ( -- * Editor
    Editor,
    newEditor,
    newEditorWithHistory,
    newEditorWithTerminal,
    readline,
    readlineWithInitial,
    editorConfig,
    modifyConfig,
    setMaxHistorySize,
    setHistoryIgnoreDups,
    setHistoryIgnoreSpace,
    setHelper,
    getHelper,
    bindSequence,
    unbindSequence,
    loadHistory,
    saveHistory,
    appendHistory,
    addHistoryEntry,
    clearHistory,
    editorHistory,
    dimensions,
    editorClearScreen,
    editorExternalPrinter,
    editorSetCursorVisibility,
    lastAcceptCursor,

    -- * Re-exports
    module System.Console.Rustyline.Binding,
    module System.Console.Rustyline.Completion,
    module System.Console.Rustyline.Config,
    module System.Console.Rustyline.Context,
    module System.Console.Rustyline.Error,
    module System.Console.Rustyline.Helper,
    module System.Console.Rustyline.Highlight,
    module System.Console.Rustyline.Hint,
    module System.Console.Rustyline.History,
    module System.Console.Rustyline.Keymap.Types,
    module System.Console.Rustyline.Keys,
    module System.Console.Rustyline.Prompt,
    module System.Console.Rustyline.Validate,
    ExternalPrinter (..),
    applyBackspaceDirect,

    -- * Internals (exposed for tests)
    readlineDirectWith,
    completeLine,
  )
where

import Control.Exception (IOException, bracket, catch, finally, throwIO, try)
import Control.Monad (forM_, void, when)
import Data.IORef
import Data.Maybe (isJust)
import Data.Text (Text)
import Data.Text qualified as T
import Data.Text.IO qualified as TIO
import System.Console.Rustyline.Binding
import System.Console.Rustyline.Command
import System.Console.Rustyline.Completion
import System.Console.Rustyline.Config
import System.Console.Rustyline.Context
import System.Console.Rustyline.Edit
import System.Console.Rustyline.Error
import System.Console.Rustyline.Helper
import System.Console.Rustyline.Highlight
import System.Console.Rustyline.Hint
import System.Console.Rustyline.History
import System.Console.Rustyline.Internal.Text (lenB)
import System.Console.Rustyline.Internal.Unicode.Grapheme (graphemes)
import System.Console.Rustyline.Keymap
import System.Console.Rustyline.Keymap.Types
import System.Console.Rustyline.Keys
import System.Console.Rustyline.KillRing
import System.Console.Rustyline.Layout (Layout (..), Position (..), layoutWidth)
import System.Console.Rustyline.LineBuffer (asStr, pos, setPos, update)
import System.Console.Rustyline.Prompt
import System.Console.Rustyline.Tty
import System.Console.Rustyline.Tty.Unix (newPosixTerminal)
import System.Console.Rustyline.Validate
import System.IO (hFlush, hGetChar, hPutStr, stderr, stdin, stdout)
import System.IO.Error (isEOFError)

-- | Line editor.
data Editor = Editor
  { edTerm :: Terminal,
    edBuffer :: IORef (Maybe Buffer),
    edHistory :: AnyHistory,
    edHelper :: IORef (Maybe Helper),
    edKillRing :: IORef KillRing,
    edConfig :: IORef Config,
    edBindings :: IORef Bindings,
    -- | Cursor column before the last accept command (a test aid).
    edAcceptCursor :: IORef Int
  }

-- | An editor with a 'DefaultHistory' configured from the config.
newEditor :: Config -> IO Editor
newEditor config = do
  history <- fileHistoryWithConfig config
  newEditorWithHistory config history

-- | An editor with a custom history.
newEditorWithHistory :: (History h) => Config -> h -> IO Editor
newEditorWithHistory config history = do
  term <- newPosixTerminal config
  newEditorWithTerminal config history term

-- | An editor on a specific terminal (e.g. a scripted one for tests).
newEditorWithTerminal :: (History h) => Config -> h -> Terminal -> IO Editor
newEditorWithTerminal config history term =
  Editor term
    <$> newIORef Nothing
    <*> pure (AnyHistory history)
    <*> newIORef Nothing
    <*> newIORef (newKillRing 60)
    <*> newIORef config
    <*> newIORef emptyBindings
    <*> newIORef 0

editorConfig :: Editor -> IO Config
editorConfig = readIORef . edConfig

modifyConfig :: Editor -> (Config -> Config) -> IO ()
modifyConfig ed = modifyIORef' (edConfig ed)

setMaxHistorySize :: Editor -> Int -> IO ()
setMaxHistorySize ed n = do
  modifyConfig ed (\c -> c {maxHistorySize = n})
  historySetMaxLen (edHistory ed) n

setHistoryIgnoreDups :: Editor -> Bool -> IO ()
setHistoryIgnoreDups ed yes = do
  modifyConfig ed (withHistoryIgnoreDups yes)
  historySetIgnoreDups (edHistory ed) yes

setHistoryIgnoreSpace :: Editor -> Bool -> IO ()
setHistoryIgnoreSpace ed yes = do
  modifyConfig ed (\c -> c {historyIgnoreSpace = yes})
  historySetIgnoreSpace (edHistory ed) yes

-- | Register the helper used for completion, hints, highlighting and
--   validation.
setHelper :: Editor -> Maybe Helper -> IO ()
setHelper ed = writeIORef (edHelper ed)

getHelper :: Editor -> IO (Maybe Helper)
getHelper = readIORef . edHelper

-- | Bind a sequence to a handler, returning the previous one.
bindSequence :: Editor -> Event -> EventHandler -> IO (Maybe EventHandler)
bindSequence ed evt handler =
  atomicModifyIORef' (edBindings ed) (swap . insertBinding evt handler)

-- | Remove the binding for the sequence.
unbindSequence :: Editor -> Event -> IO (Maybe EventHandler)
unbindSequence ed evt =
  atomicModifyIORef' (edBindings ed) (swap . removeBinding evt)

swap :: (a, b) -> (b, a)
swap (a, b) = (b, a)

loadHistory, saveHistory, appendHistory :: Editor -> FilePath -> IO ()
loadHistory ed = historyLoad (edHistory ed)
saveHistory ed = historySave (edHistory ed)
appendHistory ed = historyAppend (edHistory ed)

addHistoryEntry :: Editor -> Text -> IO Bool
addHistoryEntry ed = historyAdd (edHistory ed)

clearHistory :: Editor -> IO ()
clearHistory ed = historyClear (edHistory ed)

editorHistory :: Editor -> AnyHistory
editorHistory = edHistory

-- | Width and height of the output terminal, if it is one.
dimensions :: Editor -> IO (Maybe (Int, Int))
dimensions ed
  | termIsOutputTty (edTerm ed) = do
      out <- createWriter (edTerm ed) =<< editorConfig ed
      cols <- getColumns out
      rows <- getRows out
      pure (Just (cols, rows))
  | otherwise = pure Nothing

editorClearScreen :: Editor -> IO ()
editorClearScreen ed =
  when (termIsOutputTty (edTerm ed)) $ do
    out <- createWriter (edTerm ed) =<< editorConfig ed
    clearScreen out

editorExternalPrinter :: Editor -> IO ExternalPrinter
editorExternalPrinter = createExternalPrinter . edTerm

-- | Change cursor visibility; returns the action restoring it, if any.
editorSetCursorVisibility :: Editor -> Bool -> IO (Maybe (IO ()))
editorSetCursorVisibility ed = setCursorVisibility (edTerm ed)

lastAcceptCursor :: Editor -> IO Int
lastAcceptCursor = readIORef . edAcceptCursor

-- | Read a line, displaying the prompt. Uses terminal-style interaction when
--   input is a terminal, file-style interaction otherwise (or when the
--   terminal is not supported).
readline :: Editor -> Prompt -> IO (Either ReadlineError Text)
readline ed p = readlineWith ed p Nothing

-- | Like 'readline', with the input area pre-populated: the first text is
--   left of the cursor, the second right of it.
readlineWithInitial :: Editor -> Prompt -> (Text, Text) -> IO (Either ReadlineError Text)
readlineWithInitial ed p initial = readlineWith ed p (Just initial)

readlineWith :: Editor -> Prompt -> Maybe (Text, Text) -> IO (Either ReadlineError Text)
readlineWith ed p initial = do
  r <- try (try body)
  pure $ case r of
    Left e -> Left e
    Right (Left ioe) -> Left (ReadlineIo ioe)
    Right (Right line) -> Right line
  where
    term = edTerm ed
    body :: IO Text
    body
      | termIsUnsupported term = do
          -- Write prompt and flush it to stdout
          TIO.putStr (promptRaw p)
          hFlush stdout
          readlineDirect =<< readIORef (edHelper ed)
      | termIsInputTty term = do
          config <- editorConfig ed
          userInput <-
            bracket (enableRawMode term config) fst $ \(originalMode, keyMap) -> do
              userInput <- try @ReadlineError (readlineEdit ed p initial originalMode keyMap)
              when (autoAddHistory config) $
                forM_ userInput (void . addHistoryEntry ed)
              pure userInput
          writeln term
          either throwIO pure userInput
      | otherwise = readlineDirect =<< readIORef (edHelper ed) -- Not a tty: read from file / pipe.

-- | Handle backspace characters in a direct input.
applyBackspaceDirect :: Text -> Text
applyBackspaceDirect = T.concat . reverse . foldl' step [] . graphemes
  where
    step acc "\b" = drop 1 acc -- remove the last grapheme
    step acc g = g : acc

readlineDirect :: Maybe Helper -> IO Text
readlineDirect helper =
  readlineDirectWith (stdinLine "") (hPutStr stderr . T.unpack) (helperValidator <$> helper)
  where
    -- a line with its newline, if any ('Nothing' at end of file)
    stdinLine acc = do
      r <- try @IOException (hGetChar stdin)
      case r of
        Left e
          | isEOFError e -> pure (if null acc then Nothing else Just (T.pack (reverse acc)))
          | otherwise -> throwIO e
        Right '\n' -> pure (Just (T.pack (reverse ('\n' : acc))))
        Right c -> stdinLine (c : acc)

-- | Line-at-a-time input for non-terminals: reads lines (each with its
--   trailing newline, if any) until the validator accepts the input.
--   Messages go to the writer.
readlineDirectWith :: IO (Maybe Text) -> (Text -> IO ()) -> Maybe Validator -> IO Text
readlineDirectWith readLine' writer validator = loop ""
  where
    loop input0 =
      readLine' >>= \case
        Nothing -> throwIO Eof
        Just raw -> do
          -- Remove trailing newline
          let (noLF, trailingN) = maybe (raw, False) (,True) (T.stripSuffix "\n" raw)
              (line, trailingR)
                | trailingN = maybe (noLF, False) (,True) (T.stripSuffix "\r" noLF)
                | otherwise = (noLF, False)
              input = applyBackspaceDirect (input0 <> line)
          case validator of
            Nothing -> pure input
            Just v ->
              validate v (ValidationContext input) >>= \case
                Valid msg -> input <$ mapM_ writer msg
                Invalid (Just msg) -> writer msg >> loop input
                -- Add newline and keep on taking input
                Incomplete ->
                  loop (input <> (if trailingR then "\r" else "") <> (if trailingN then "\n" else ""))
                Invalid Nothing -> loop input

-- | Handles reading and editing the readline buffer.
readlineEdit :: Editor -> Prompt -> Maybe (Text, Text) -> IO () -> KeyMap -> IO Text
readlineEdit ed p initial originalMode termKeyMap = do
  config <- editorConfig ed
  let term = edTerm ed
  out <- createWriter term config
  modifyIORef' (edKillRing ed) resetKillRing
  helper <- readIORef (edHelper ed)
  s <- newState out p helper (edHistory ed)
  is <- newInputState config (edBindings ed) (termTestBindings term)
  forM_ initial $ \(left, right) ->
    withLine s (update (left <> right) (lenB left))
  buffer <- atomicModifyIORef' (edBuffer ed) (Nothing,)
  rdr <- createReader term buffer config termKeyMap
  (`finally` releaseReader rdr) $ do
    when (termIsOutputTty term && checkCursorPosition config) $
      moveCursorAtLeftmostS s rdr `catch` \case
        ReadlineSignal SigResize -> updateSize out
        e -> throwIO e
    refreshLineS s
    let loop = do
          cmd0 <- stateNextCmd s is rdr False False
          when (shouldResetKillRing cmd0) $ modifyIORef' (edKillRing ed) resetKillRing
          -- First trigger commands that need extra input
          next <-
            if cmd0 == Complete && isJust helper
              then completeLine rdr s is config
              else pure (Just cmd0)
          next' <- case next of
            Just ReverseSearchHistory -> reverseIncrementalSearch rdr s is
            other -> pure other
          case next' of
            Nothing -> loop
            Just Suspend -> do
              originalMode
              suspendProcess term
              _ <- enableRawMode term config -- TODO original_mode may have changed
              updateSize out -- window may have been resized
              refreshLineS s
              loop
            Just QuotedInsert -> do
              c <- nextChar rdr
              editInsert s c 1
              loop
            Just cmd -> do
              when (isAcceptCmd cmd) $
                writeIORef (edAcceptCursor ed) . posCol . layoutCursor =<< readIORef (stLayout s)
              -- Execute things can be done solely on a state object
              execute cmd s is (edKillRing ed) config >>= \case
                Proceed -> loop
                Submit -> pure ()
    loop
    -- Move to end, in case cursor was in the middle of the line, so that
    -- next thing application prints goes after the input
    editMoveBufferEnd s ForcedRefresh
    writeIORef (edBuffer ed) =<< unbuffer rdr
    asStr <$> readLine s
  where
    isAcceptCmd = \case
      AcceptLine -> True
      Newline -> True
      AcceptOrInsertLine _ -> True
      _ -> False

-- | Completes the line/word.
completeLine :: RawReader -> State -> InputState -> Config -> IO (Maybe Cmd)
completeLine rdr s is config = case stHelper s of
  Nothing -> pure Nothing
  Just helper -> do
    let completer = helperCompleter helper
        out = stOut s
    lb0 <- readLine s
    ctx <- stateContext s
    -- get a list of completions
    (start, candidates) <- complete completer (asStr lb0) (pos lb0) ctx
    let n = length candidates
    if
      | null candidates -> Nothing <$ beep out
      | completionType config == Circular -> do
          mark <- changesBegin s
          -- Save the current edited line before overwriting it
          let backup' = asStr lb0
              backupPos = pos lb0
              cycleLoop i = do
                -- Show completion or original buffer
                if i < n
                  then withLine s (updateCompletion completer start (pairReplacement (candidates !! i)))
                  else -- Restore current edited line
                    withLine s (update backup' backupPos)
                refreshLineS s
                cmd <- stateNextCmd s is rdr True True
                case cmd of
                  Complete -> do
                    let i' = (i + 1) `mod` (n + 1) -- Circular
                    when (i' == n) (beep out)
                    cycleLoop i'
                  CompleteBackward
                    | i == 0 -> beep out >> cycleLoop n -- Circular
                    | otherwise -> cycleLoop ((i - 1) `mod` (n + 1))
                  Abort -> do
                    -- Re-show original buffer
                    when (i < n) $ do
                      withLine s (update backup' backupPos)
                      refreshLineS s
                    changesTruncate s mark
                    pure Nothing
                  _ -> do
                    _ <- changesEnd s
                    pure (Just cmd)
          cycleLoop 0
      | otherwise -> do
          -- List
          forM_ (longestCommonPrefix candidates) $ \lcp -> do
            -- if we can extend the item, extend it
            p <- pos <$> readLine s
            when (lenB lcp > p - start || n == 1) $ do
              withLine s (updateCompletion completer start lcp)
              refreshLineS s
          -- beep if ambiguous
          if n <= 1
            then pure Nothing
            else do
              beep out
              cmd0 <-
                if completionShowAllIfAmbiguous config
                  then pure Complete
                  else -- we can't complete any further, wait for second tab
                    stateNextCmd s is rdr True True
              -- if any character other than tab, pass it to the main loop
              if cmd0 /= Complete
                then pure (Just cmd0)
                else do
                  -- move cursor to EOL to avoid overwriting the command line
                  savePos <- pos <$> readLine s
                  editMoveEnd s
                  _ <- withLine s (setPos savePos)
                  -- we got a second tab, maybe show list of possible completions
                  let asked = n > completionPromptLimit config
                  showCompletions <-
                    if asked
                      then do
                        writeAndFlush out ("\nDisplay all " <> T.pack (show n) <> " possibilities? (y or n)")
                        let answers = [SelfInsert 1 'y', SelfInsert 1 'Y', SelfInsert 1 'n', SelfInsert 1 'N', Kill (BackwardChar 1)]
                            ask cmd
                              | cmd `elem` answers = pure cmd
                              | otherwise = stateNextCmd s is rdr False True >>= ask
                        answer <- ask cmd0
                        pure (answer `elem` [SelfInsert 1 'y', SelfInsert 1 'Y'])
                      else pure True
                  if showCompletions
                    then pageCompletions rdr s is candidates
                    else do
                      when asked $ -- erase Display all ... possibilities
                        modifyIORef' (stLayout s) (\l -> l {layoutEnd = (layoutEnd l) {posRow = posRow (layoutEnd l) + 1}})
                      refreshLineS s
                      pure Nothing

pageCompletions :: RawReader -> State -> InputState -> [Pair] -> IO (Maybe Cmd)
pageCompletions rdr s is candidates = do
  let out = stOut s
      minColPad = 2
  layout <- readIORef (stLayout s)
  cols <- getColumns out
  rows <- getRows out
  let maxWidth = min cols (maximum (map (layoutWidth layout . pairDisplay) candidates) + minColPad)
      numCols = max 1 (cols `div` maxWidth)
      nbc = length candidates
      numRows = (nbc + numCols - 1) `div` numCols
      stopKeys = [SelfInsert 1 c | c <- "yYnNqQ "] ++ [Kill (BackwardChar 1), AcceptLine, Newline]
      isStop = \case
        AcceptOrInsertLine _ -> True
        c -> c `elem` stopKeys
      waitStop cmd
        | isStop cmd = pure cmd
        | otherwise = stateNextCmd s is rdr False True >>= waitStop
      rowLoop row pauseRow
        | row >= numRows = pure ()
        | otherwise = do
            continue <-
              if row /= pauseRow
                then pure (Just pauseRow)
                else do
                  writeAndFlush out "\n--More--"
                  cmd <- waitStop Noop
                  pure $ case cmd of
                    SelfInsert 1 c | c `elem` ("yY " :: String) -> Just (pauseRow + rows - 1)
                    AcceptLine -> Just (pauseRow + 1)
                    Newline -> Just (pauseRow + 1)
                    AcceptOrInsertLine _ -> Just (pauseRow + 1)
                    _ -> Nothing
            case continue of
              Nothing -> pure ()
              Just pauseRow' -> do
                writeAndFlush out "\n"
                cells <- mapM (cell row) [0 .. numCols - 1]
                writeAndFlush out (T.concat cells)
                rowLoop (row + 1) pauseRow'
      cell row col = do
        let i = col * numRows + row
        if i >= nbc
          then pure ""
          else do
            let c = pairDisplay (candidates !! i)
                width = layoutWidth layout c
            shown <- case highlighter s of
              Just h -> highlightCandidate h c List
              Nothing -> pure c
            let pad
                  | (col + 1) * numRows + row < nbc = T.replicate (maxWidth - width) " "
                  | otherwise = ""
            pure (shown <> pad)
  rowLoop 0 (rows - 1)
  writeAndFlush out "\n"
  repaint s RefreshMin
  pure Nothing

-- | Incremental search.
reverseIncrementalSearch :: RawReader -> State -> InputState -> IO (Maybe Cmd)
reverseIncrementalSearch rdr s is = do
  let history = stHistory s
  size <- historyLen history
  if size == 0
    then pure Nothing
    else do
      mark <- changesBegin s
      -- Save the current edited line (and cursor position) before
      -- overwriting it
      lb0 <- readLine s
      let backup' = asStr lb0
          backupPos = pos lb0
          loop searchBuf historyIdx direction success = do
            let promptText
                  | success = "(reverse-i-search)`" <> searchBuf <> "': "
                  | otherwise = "(failed reverse-i-search)`" <> searchBuf <> "': "
            refreshPromptAndLine s promptText
            cmd <- stateNextCmd s is rdr True True
            let search buf idx dir = do
                  r <- historySearch history buf idx dir
                  case r of
                    Just sr -> do
                      withLine s (update (srEntry sr) (srPos sr))
                      loop buf (srIdx sr) dir True
                    Nothing -> loop buf idx dir False
            case cmd of
              SelfInsert _ c -> search (T.snoc searchBuf c) historyIdx direction
              Kill (BackwardChar _) -> loop (if T.null searchBuf then searchBuf else T.init searchBuf) historyIdx direction success
              ReverseSearchHistory
                | historyIdx > 0 -> search searchBuf (historyIdx - 1) SearchReverse
                | otherwise -> loop searchBuf historyIdx SearchReverse False
              ForwardSearchHistory
                | historyIdx < size - 1 -> search searchBuf (historyIdx + 1) SearchForward
                | otherwise -> loop searchBuf historyIdx SearchForward False
              Abort -> do
                -- Restore current edited line (before search)
                withLine s (update backup' backupPos)
                refreshLineS s
                changesTruncate s mark
                pure Nothing
              Move _ -> do
                refreshLineS s -- restore prompt
                done cmd
              _ -> done cmd
          done cmd = do
            _ <- changesEnd s
            pure (Just cmd)
      loop "" (size - 1) SearchReverse True
