-- | Bindings from keys to commands for Emacs and Vi modes (@keymap.rs@).
module System.Console.Rustyline.Keymap
  ( module System.Console.Rustyline.Keymap.Types,
    Refresher (..),
    InputState (..),
    newInputState,
    isEmacsMode,
    isViCmdMode,
    nextCmd,
  )
where

import Control.Exception (throwIO)
import Control.Monad (when)
import Data.Char (digitToInt, isDigit)
import Data.IORef
import Data.Text (Text)
import Data.Text qualified as T
import System.Console.Rustyline.Binding
import System.Console.Rustyline.Config (Config (..), EditMode (..))
import System.Console.Rustyline.Highlight (CmdKind)
import System.Console.Rustyline.Keymap.Types
import System.Console.Rustyline.Keys
import System.Console.Rustyline.Tty
import Prelude hiding (Word)

-- | Indirect access to the editor, given to the keymap.
data Refresher = Refresher
  { -- | Rewrite the currently edited line according to the buffer content,
    --   cursor position and number of columns of the terminal.
    rfRefreshLine :: IO (),
    -- | Same as 'rfRefreshLine' with a specific message instead of the hint.
    rfRefreshLineWithMsg :: Maybe Text -> CmdKind -> IO (),
    -- | Same as 'rfRefreshLine' but with a dynamic prompt.
    rfRefreshPromptAndLine :: Text -> IO (),
    -- | Vi only, switch to insert mode.
    rfDoingInsert :: IO (),
    -- | Vi only, switch to command mode.
    rfDoneInserting :: IO (),
    -- | Vi only, last text inserted.
    rfLastInsert :: IO (Maybe Text),
    rfIsCursorAtEnd :: IO Bool,
    rfHasHint :: IO Bool,
    -- | The hint text shown after the cursor.
    rfHintText :: IO (Maybe Text),
    -- | The currently edited line.
    rfLine :: IO Text,
    -- | Current cursor position (byte position).
    rfPos :: IO Int,
    -- | Display a message above the currently edited line.
    rfExternalPrint :: Text -> IO ()
  }

-- | Transforms keys into commands based on the current input mode.
data InputState = InputState
  { isMode :: EditMode,
    isCustomBindings :: IORef Bindings,
    isTestBindings :: Bool,
    -- | vi only?
    isInputMode :: IORef InputMode,
    -- | numeric arguments: http://web.mit.edu/gnu/doc/html/rlman_1.html#SEC7
    isNumArgs :: IORef Int,
    -- | vi only
    isLastCmd :: IORef Cmd,
    -- | vi only
    isLastCharSearch :: IORef (Maybe CharSearch)
  }

newInputState :: Config -> IORef Bindings -> Bool -> IO InputState
newInputState config bindings testBindings =
  InputState (editMode config) bindings testBindings
    <$> newIORef InsertMode
    <*> newIORef 0
    <*> newIORef Noop
    <*> newIORef Nothing

isEmacsMode :: InputState -> Bool
isEmacsMode is = isMode is == Emacs

isViCmdMode :: InputState -> IO Bool
isViCmdMode is = do
  im <- readIORef (isInputMode is)
  pure (im == CommandMode && isMode is == Vi)

setInputMode :: InputState -> InputMode -> IO ()
setInputMode is = writeIORef (isInputMode is)

-- | Rust's @i16@ saturating arithmetic.
saturate :: Int -> Int
saturate = max (-32768) . min 32767

-- | Parse user input into one command. @singleEscAbort@ is used in emacs
--   mode when a single ESC is expected to abort the current action.
nextCmd :: InputState -> RawReader -> Refresher -> Bool -> Bool -> IO Cmd
nextCmd is rdr wrt singleEscAbort0 ignoreExternalPrint = do
  let singleEscAbort = case isMode is of
        Emacs -> singleEscAbort0
        Vi -> False
  key <-
    if ignoreExternalPrint
      then nextKey rdr singleEscAbort
      else
        let loop =
              waitForInput rdr singleEscAbort >>= \case
                KeyPress k -> pure k
                ExternalPrint msg -> rfExternalPrint wrt msg >> loop
                Timeout _ -> loop
         in loop
  inputMode <- readIORef (isInputMode is)
  case isMode is of
    Emacs -> emacs is rdr wrt key
    Vi
      | inputMode /= CommandMode -> viInsert is rdr wrt key
      | otherwise -> viCommand is rdr wrt key

-- | Terminal peculiar binding.
termBinding :: RawReader -> Refresher -> KeyEvent -> IO (Maybe Cmd)
termBinding rdr wrt key = case findBinding rdr key of
  Just EndOfFile -> do
    line <- rfLine wrt
    pure (if T.null line then Just EndOfFile else Nothing) -- Eof only if line is empty
  cmd -> pure cmd

digitValue :: Char -> Int
digitValue = digitToInt

emacsDigitArgument :: InputState -> RawReader -> Refresher -> Char -> IO KeyEvent
emacsDigitArgument is rdr wrt digit = do
  writeIORef (isNumArgs is) (if digit == '-' then -1 else digitValue digit)
  let loop = do
        numArgs <- readIORef (isNumArgs is)
        rfRefreshPromptAndLine wrt ("(arg: " <> T.pack (show numArgs) <> ") ")
        key <- nextKey rdr True
        case key of
          KeyEvent (KChar d) m
            | isDigit d && (m == NONE || m == ALT) -> do
                if numArgs == -1
                  then writeIORef (isNumArgs is) (saturate (numArgs * digitValue d))
                  else
                    when (abs numArgs < 1000) $ -- shouldn't ever need more than 4 digits
                      writeIORef (isNumArgs is) (saturate (numArgs * 10 + digitValue d))
                loop
          KeyEvent (KChar '-') m | m == NONE || m == ALT -> loop
          _ -> key <$ rfRefreshLine wrt
  loop

redoWith :: Refresher -> Cmd -> Maybe RepeatCount -> IO Cmd
redoWith wrt cmd n = redoCmd cmd n <$> rfLastInsert wrt

emacs :: InputState -> RawReader -> Refresher -> KeyEvent -> IO Cmd
emacs is rdr wrt key0 = do
  key <- case key0 of
    KeyEvent (KChar d) ALT | d == '-' || isDigit d -> emacsDigitArgument is rdr wrt d
    _ -> pure key0
  (n, positive) <- emacsNumArgs is -- consume them in all cases
  let evt = keySeq key
  custom <- customBinding is wrt evt n positive
  term <- termBinding rdr wrt key
  case (custom, term) of
    (Just cmd, _)
      | isRepeatable cmd -> redoWith wrt cmd (Just n)
      | otherwise -> pure cmd
    (Nothing, Just cmd) -> pure cmd
    _ -> do
      hasHint <- rfHasHint wrt
      atEnd <- rfIsCursorAtEnd wrt
      let dir :: a -> a -> a
          dir fwd bwd = if positive then fwd else bwd
          wordBack = BackwardWord n WordEmacs
          wordFwd = ForwardWord n AfterEnd WordEmacs
      case key of
        KeyEvent (KChar c) NONE -> pure (if positive then SelfInsert n c else Unknown)
        KeyEvent (KChar 'A') CTRL -> pure (Move BeginningOfLine)
        KeyEvent (KChar 'B') CTRL -> pure (Move (dir (BackwardChar n) (ForwardChar n)))
        KeyEvent (KChar 'E') CTRL -> pure (Move EndOfLine)
        KeyEvent (KChar 'F') CTRL -> pure (Move (dir (ForwardChar n) (BackwardChar n)))
        KeyEvent (KChar 'G') m | m == CTRL || m == CTRL_ALT -> pure Abort
        ESC -> pure Abort
        KeyEvent (KChar 'H') CTRL -> pure (Kill (dir (BackwardChar n) (ForwardChar n)))
        BACKSPACE -> pure (Kill (dir (BackwardChar n) (ForwardChar n)))
        KeyEvent KBackTab NONE -> pure CompleteBackward
        KeyEvent (KChar 'I') CTRL -> pure (dir Complete CompleteBackward)
        KeyEvent KTab NONE -> pure (dir Complete CompleteBackward)
        -- Don't complete hints when the cursor is not at the end of a line
        KeyEvent KRight NONE | hasHint && atEnd -> pure CompleteHint
        KeyEvent (KChar 'K') CTRL -> pure (Kill (dir EndOfLine BeginningOfLine))
        KeyEvent (KChar 'L') CTRL -> pure ClearScreen
        KeyEvent (KChar 'N') CTRL -> pure NextHistory
        KeyEvent (KChar 'P') CTRL -> pure PreviousHistory
        KeyEvent (KChar 'X') CTRL -> do
          (seqCmd, evt') <- customSeqBinding is rdr wrt evt n positive
          case seqCmd of
            Just cmd -> pure cmd
            Nothing -> do
              sndKey <- case evt' of
                -- we may have already read the second key in customSeqBinding
                KeySeq (_ : k : _) -> pure k
                _ -> nextKey rdr True
              pure $ case sndKey of
                KeyEvent (KChar 'G') CTRL -> Abort
                ESC -> Abort
                KeyEvent (KChar 'U') CTRL -> Undo n
                KeyEvent KBackspace NONE -> Kill (dir BeginningOfLine EndOfLine)
                _ -> Unknown
        -- character-search, character-search-backward
        KeyEvent (KChar ']') m | m == CTRL || m == CTRL_ALT -> do
          ch <- nextKey rdr False
          pure $ case ch of
            KeyEvent (KChar c) NONE ->
              let withAlt = hasMods m ALT
                  cs
                    | positive = if withAlt then CSBackward c else CSForwardBefore c
                    | withAlt = CSForwardBefore c
                    | otherwise = CSBackward c
               in Move (ViCharSearch n cs)
            _ -> Unknown
        KeyEvent KBackspace ALT -> pure (Kill (dir wordBack wordFwd))
        KeyEvent (KChar '<') ALT -> pure BeginningOfHistory
        KeyEvent (KChar '>') ALT -> pure EndOfHistory
        KeyEvent (KChar c) ALT | c == 'B' || c == 'b' -> pure (Move (dir wordBack wordFwd))
        KeyEvent KLeft ALT -> pure (Move (dir wordBack wordFwd))
        KeyEvent KLeft CTRL -> pure (Move (dir wordBack wordFwd))
        KeyEvent (KChar c) ALT | c == 'C' || c == 'c' -> pure CapitalizeWord
        KeyEvent (KChar c) ALT | c == 'D' || c == 'd' -> pure (Kill (dir wordFwd wordBack))
        KeyEvent (KChar c) ALT | c == 'F' || c == 'f' -> pure (Move (dir wordFwd wordBack))
        KeyEvent KRight ALT -> pure (Move (dir wordFwd wordBack))
        KeyEvent KRight CTRL -> pure (Move (dir wordFwd wordBack))
        KeyEvent (KChar c) ALT | c == 'L' || c == 'l' -> pure DowncaseWord
        KeyEvent (KChar c) ALT | c == 'T' || c == 't' -> pure (TransposeWords n)
        -- TODO ESC-R (r): Undo all changes made to this line.
        KeyEvent (KChar c) ALT | c == 'U' || c == 'u' -> pure UpcaseWord
        KeyEvent (KChar c) ALT | c == 'Y' || c == 'y' -> pure YankPop
        _ -> common is rdr wrt evt key n positive

viArgDigit :: InputState -> RawReader -> Refresher -> Char -> IO KeyEvent
viArgDigit is rdr wrt digit = do
  writeIORef (isNumArgs is) (digitValue digit)
  let loop = do
        numArgs <- readIORef (isNumArgs is)
        rfRefreshPromptAndLine wrt ("(arg: " <> T.pack (show numArgs) <> ") ")
        key <- nextKey rdr False
        case key of
          KeyEvent (KChar d) NONE | isDigit d -> do
            when (abs numArgs < 1000) $ -- shouldn't ever need more than 4 digits
              writeIORef (isNumArgs is) (saturate (numArgs * 10 + digitValue d))
            loop
          _ -> key <$ rfRefreshLine wrt
  loop

viCommand :: InputState -> RawReader -> Refresher -> KeyEvent -> IO Cmd
viCommand is rdr wrt key0 = do
  key <- case key0 of
    KeyEvent (KChar d) NONE | d >= '1' && d <= '9' -> viArgDigit is rdr wrt d
    _ -> pure key0
  noNumArgs <- (== 0) <$> readIORef (isNumArgs is)
  n <- viNumArgs is -- consume them in all cases
  let evt = keySeq key
      redoN cmd = redoWith wrt cmd (if noNumArgs then Nothing else Just n)
      insertMode = setInputMode is InsertMode
      motion k f = maybe Unknown f <$> viCmdMotion is rdr wrt k n
  custom <- customBinding is wrt evt n True
  term <- termBinding rdr wrt key
  case (custom, term) of
    (Just cmd, _)
      | isRepeatable cmd -> redoN cmd
      | otherwise -> pure cmd
    (Nothing, Just cmd) -> pure cmd
    _ -> do
      lastCharSearch <- readIORef (isLastCharSearch is)
      cmd <- case key of
        KeyEvent (KChar '$') NONE -> pure (Move EndOfLine)
        KeyEvent KEnd NONE -> pure (Move EndOfLine)
        KeyEvent (KChar '.') NONE -> do
          -- vi-redo (repeat last command)
          lastCmd <- readIORef (isLastCmd is)
          if not (isRepeatable lastCmd) then pure Noop else redoN lastCmd
        -- TODO '%': Move to the corresponding opening/closing bracket
        KeyEvent (KChar '0') NONE -> pure (Move BeginningOfLine)
        KeyEvent (KChar '^') NONE -> pure (Move ViFirstPrint)
        KeyEvent (KChar 'a') NONE -> do
          -- vi-append-mode
          insertMode
          rfDoingInsert wrt
          pure (Move (ForwardChar n))
        KeyEvent (KChar 'A') NONE -> do
          -- vi-append-eol
          insertMode
          rfDoingInsert wrt
          pure (Move EndOfLine)
        KeyEvent (KChar 'b') NONE -> pure (Move (BackwardWord n WordVi)) -- vi-prev-word
        KeyEvent (KChar 'B') NONE -> pure (Move (BackwardWord n WordBig))
        KeyEvent (KChar 'c') NONE -> do
          insertMode
          motion key (`Replace` Nothing)
        KeyEvent (KChar 'C') NONE -> do
          insertMode
          pure (Replace EndOfLine Nothing)
        KeyEvent (KChar 'd') NONE -> motion key Kill
        KeyEvent (KChar 'D') NONE -> pure (Kill EndOfLine)
        KeyEvent (KChar 'K') CTRL -> pure (Kill EndOfLine)
        KeyEvent (KChar 'e') NONE -> pure (Move (ForwardWord n BeforeEnd WordVi))
        KeyEvent (KChar 'E') NONE -> pure (Move (ForwardWord n BeforeEnd WordBig))
        KeyEvent (KChar 'i') NONE -> do
          -- vi-insertion-mode
          insertMode
          rfDoingInsert wrt
          pure Noop
        KeyEvent (KChar 'I') NONE -> do
          -- vi-insert-beg
          insertMode
          rfDoingInsert wrt
          pure (Move BeginningOfLine)
        KeyEvent (KChar c) NONE | c `elem` ("fFtT" :: String) -> do
          -- vi-char-search
          cs <- viCharSearch is rdr c
          pure (maybe Unknown (Move . ViCharSearch n) cs)
        KeyEvent (KChar ';') NONE -> pure (maybe Noop (Move . ViCharSearch n) lastCharSearch)
        KeyEvent (KChar ',') NONE ->
          pure (maybe Noop (Move . ViCharSearch n . oppositeCharSearch) lastCharSearch)
        -- TODO 'G': Move to the history line n
        KeyEvent (KChar 'p') NONE -> pure (Yank n After) -- vi-put
        KeyEvent (KChar 'P') NONE -> pure (Yank n Before) -- vi-put
        KeyEvent (KChar 'r') NONE -> do
          -- vi-replace-char
          ch <- nextKey rdr False
          pure $ case ch of
            KeyEvent (KChar c) NONE -> ReplaceChar n c
            ESC -> Noop
            _ -> Unknown
        KeyEvent (KChar 'R') NONE -> do
          -- vi-replace-mode (overwrite-mode)
          setInputMode is ReplaceMode
          pure (Replace (ForwardChar 0) Nothing)
        KeyEvent (KChar 's') NONE -> do
          -- vi-substitute-char
          insertMode
          pure (Replace (ForwardChar n) Nothing)
        KeyEvent (KChar 'S') NONE -> do
          -- vi-substitute-line
          insertMode
          pure (Replace WholeLine Nothing)
        KeyEvent (KChar 'u') NONE -> pure (Undo n)
        KeyEvent (KChar 'w') NONE -> pure (Move (ForwardWord n Start WordVi)) -- vi-next-word
        KeyEvent (KChar 'W') NONE -> pure (Move (ForwardWord n Start WordBig)) -- vi-next-word
        -- TODO move backward if eol
        KeyEvent (KChar 'x') NONE -> pure (Kill (ForwardChar n)) -- vi-delete
        KeyEvent (KChar 'X') NONE -> pure (Kill (BackwardChar n)) -- vi-rubout
        KeyEvent (KChar 'y') NONE -> motion key ViYankTo
        KeyEvent (KChar 'h') NONE -> pure (Move (BackwardChar n))
        KeyEvent (KChar 'H') CTRL -> pure (Move (BackwardChar n))
        BACKSPACE -> pure (Move (BackwardChar n))
        KeyEvent (KChar 'G') CTRL -> pure Abort
        KeyEvent (KChar c) NONE | c == 'l' || c == ' ' -> pure (Move (ForwardChar n))
        KeyEvent (KChar 'L') CTRL -> pure ClearScreen
        KeyEvent (KChar c) NONE | c == '+' || c == 'j' -> pure (LineDownOrNextHistory n)
        -- TODO: move to the start of the line.
        KeyEvent (KChar 'N') CTRL -> pure NextHistory
        KeyEvent (KChar c) NONE | c == '-' || c == 'k' -> pure (LineUpOrPreviousHistory n)
        -- TODO: move to the start of the line.
        KeyEvent (KChar 'P') CTRL -> pure PreviousHistory
        KeyEvent (KChar 'R') CTRL -> do
          insertMode -- TODO Validate
          pure ReverseSearchHistory
        KeyEvent (KChar 'S') CTRL -> do
          insertMode -- TODO Validate
          pure ForwardSearchHistory
        KeyEvent (KChar '<') NONE -> motion key Dedent
        KeyEvent (KChar '>') NONE -> motion key Indent
        ESC -> pure Noop
        _ -> common is rdr wrt evt key n True
      when (isRepeatableChange cmd) $ writeIORef (isLastCmd is) cmd
      pure cmd

viInsert :: InputState -> RawReader -> Refresher -> KeyEvent -> IO Cmd
viInsert is rdr wrt key = do
  let evt = keySeq key
  custom <- customBinding is wrt evt 0 True
  term <- termBinding rdr wrt key
  case (custom, term) of
    (Just cmd, _)
      | isRepeatable cmd -> redoWith wrt cmd Nothing
      | otherwise -> pure cmd
    (Nothing, Just cmd) -> pure cmd
    _ -> do
      inputMode <- readIORef (isInputMode is)
      hasHint <- rfHasHint wrt
      atEnd <- rfIsCursorAtEnd wrt
      cmd <- case key of
        KeyEvent (KChar c) NONE
          | inputMode == ReplaceMode -> pure (Overwrite c)
          | otherwise -> pure (SelfInsert 1 c)
        KeyEvent (KChar 'H') CTRL -> pure (Kill (BackwardChar 1))
        BACKSPACE -> pure (Kill (BackwardChar 1))
        KeyEvent KBackTab NONE -> pure CompleteBackward
        KeyEvent (KChar 'I') CTRL -> pure Complete
        KeyEvent KTab NONE -> pure Complete
        -- Don't complete hints when the cursor is not at the end of a line
        KeyEvent KRight NONE | hasHint && atEnd -> pure CompleteHint
        KeyEvent (KChar k) ALT -> do
          -- vi fast command mode
          setInputMode is CommandMode
          rfDoneInserting wrt
          viCommand is rdr wrt (KeyEvent (KChar k) NONE)
        ESC -> do
          -- vi-movement-mode/vi-command-mode
          setInputMode is CommandMode
          rfDoneInserting wrt
          pure (Move (BackwardChar 1))
        _ -> common is rdr wrt evt key 1 True
      when (isRepeatableChange cmd) $ do
        lastCmd <- readIORef (isLastCmd is)
        case (lastCmd, cmd) of
          (Replace _ _, SelfInsert _ _) -> pure () -- replacing...
          (SelfInsert _ _, SelfInsert _ _) -> pure () -- inserting...
          _ -> writeIORef (isLastCmd is) cmd
      pure cmd

viCmdMotion :: InputState -> RawReader -> Refresher -> KeyEvent -> RepeatCount -> IO (Maybe Movement)
viCmdMotion is rdr wrt key n0 = do
  mvt0 <- nextKey rdr False
  if mvt0 == key
    then pure (Just WholeLine)
    else do
      (mvt, n) <- case mvt0 of
        KeyEvent (KChar d) NONE | d >= '1' && d <= '9' -> do
          -- vi-arg-digit
          m <- viArgDigit is rdr wrt d
          k <- viNumArgs is
          pure (m, min repeatCountMax (k * n0))
        _ -> pure (mvt0, n0)
      lastCharSearch <- readIORef (isLastCharSearch is)
      let isC = key == KeyEvent (KChar 'c') NONE
      case mvt of
        KeyEvent (KChar '$') NONE -> pure (Just EndOfLine)
        KeyEvent (KChar '0') NONE -> pure (Just BeginningOfLine)
        KeyEvent (KChar '^') NONE -> pure (Just ViFirstPrint)
        KeyEvent (KChar 'b') NONE -> pure (Just (BackwardWord n WordVi))
        KeyEvent (KChar 'B') NONE -> pure (Just (BackwardWord n WordBig))
        KeyEvent (KChar 'e') NONE -> pure (Just (ForwardWord n AfterEnd WordVi))
        KeyEvent (KChar 'E') NONE -> pure (Just (ForwardWord n AfterEnd WordBig))
        KeyEvent (KChar c) NONE | c `elem` ("fFtT" :: String) -> do
          cs <- viCharSearch is rdr c
          pure (ViCharSearch n <$> cs)
        KeyEvent (KChar ';') NONE -> pure (ViCharSearch n <$> lastCharSearch)
        KeyEvent (KChar ',') NONE -> pure (ViCharSearch n . oppositeCharSearch <$> lastCharSearch)
        KeyEvent (KChar 'h') NONE -> pure (Just (BackwardChar n))
        KeyEvent (KChar 'H') CTRL -> pure (Just (BackwardChar n))
        BACKSPACE -> pure (Just (BackwardChar n))
        KeyEvent (KChar c) NONE | c == 'l' || c == ' ' -> pure (Just (ForwardChar n))
        KeyEvent (KChar c) NONE | c == 'j' || c == '+' -> pure (Just (LineDown n))
        KeyEvent (KChar c) NONE | c == 'k' || c == '-' -> pure (Just (LineUp n))
        -- 'cw' is 'ce'
        KeyEvent (KChar 'w') NONE
          | isC -> pure (Just (ForwardWord n AfterEnd WordVi))
          | otherwise -> pure (Just (ForwardWord n Start WordVi))
        -- 'cW' is 'cE'
        KeyEvent (KChar 'W') NONE
          | isC -> pure (Just (ForwardWord n AfterEnd WordBig))
          | otherwise -> pure (Just (ForwardWord n Start WordBig))
        _ -> pure Nothing

viCharSearch :: InputState -> RawReader -> Char -> IO (Maybe CharSearch)
viCharSearch is rdr cmd = do
  ch <- nextKey rdr False
  case ch of
    KeyEvent (KChar c) NONE -> do
      let cs = case cmd of
            'f' -> CSForward c
            't' -> CSForwardBefore c
            'F' -> CSBackward c
            _ -> CSBackwardAfter c
      writeIORef (isLastCharSearch is) (Just cs)
      pure (Just cs)
    _ -> pure Nothing

common :: InputState -> RawReader -> Refresher -> Event -> KeyEvent -> RepeatCount -> Bool -> IO Cmd
common is rdr wrt evt key n positive = do
  let dir :: a -> a -> a
      dir fwd bwd = if positive then fwd else bwd
  case key of
    KeyEvent KHome NONE -> pure (Move BeginningOfLine)
    KeyEvent KLeft NONE -> pure (Move (dir (BackwardChar n) (ForwardChar n)))
    KeyEvent (KChar 'C') CTRL | isTestBindings is -> pure Interrupt
    KeyEvent (KChar 'D') CTRL -> do
      line <- rfLine wrt
      pure $
        if isEmacsMode is && not (T.null line)
          then Kill (dir (ForwardChar n) (BackwardChar n))
          else
            if isTestBindings is || not (T.null line)
              then EndOfFile
              else Unknown
    KeyEvent KDelete NONE -> pure (Kill (dir (ForwardChar n) (BackwardChar n)))
    KeyEvent KEnd NONE -> pure (Move EndOfLine)
    KeyEvent KRight NONE -> pure (Move (dir (ForwardChar n) (BackwardChar n)))
    KeyEvent (KChar c) CTRL | c == 'J' || c == 'M' -> pure (AcceptOrInsertLine True)
    ENTER -> pure (AcceptOrInsertLine True)
    KeyEvent KDown NONE -> pure (LineDownOrNextHistory 1)
    KeyEvent KUp NONE -> pure (LineUpOrPreviousHistory 1)
    KeyEvent (KChar 'R') CTRL -> pure ReverseSearchHistory
    -- most terminals override Ctrl+S to suspend execution
    KeyEvent (KChar 'S') CTRL -> pure ForwardSearchHistory
    KeyEvent (KChar 'T') CTRL -> pure TransposeChars
    KeyEvent (KChar 'U') CTRL -> pure (Kill (dir BeginningOfLine EndOfLine))
    -- most terminals override Ctrl+Q to resume execution
    KeyEvent (KChar 'Q') CTRL -> pure QuotedInsert
    KeyEvent (KChar 'V') CTRL -> pure QuotedInsert
    KeyEvent (KChar 'W') CTRL -> pure (Kill (dir (BackwardWord n WordBig) (ForwardWord n AfterEnd WordBig)))
    KeyEvent (KChar 'Y') CTRL -> pure (if positive then Yank n Before else Unknown) -- TODO Validate
    KeyEvent (KChar '_') CTRL -> pure (Undo n)
    KeyEvent KUnknownEscSeq NONE -> pure Noop
    KeyEvent KBracketedPasteStart NONE -> Insert 1 <$> readPastedText rdr
    _ -> maybe Unknown id . fst <$> customSeqBinding is rdr wrt evt n positive

numArgs :: InputState -> IO Int
numArgs is = do
  k <- readIORef (isNumArgs is)
  writeIORef (isNumArgs is) 0
  pure (if k == 0 then 1 else k)

emacsNumArgs :: InputState -> IO (RepeatCount, Bool)
emacsNumArgs is = do
  k <- numArgs is
  pure $
    if k < 0
      then (if k == -32768 then repeatCountMax else negate k, False)
      else (k, True)

viNumArgs :: InputState -> IO RepeatCount
viNumArgs is = do
  k <- numArgs is
  if k < 0 then throwIO (userError "viNumArgs: negative count") else pure k

eventContext :: InputState -> Refresher -> IO EventContext
eventContext is wrt =
  EventContext (isMode is)
    <$> readIORef (isInputMode is)
    <*> rfHintText wrt
    <*> rfHasHint wrt
    <*> rfLine wrt
    <*> rfPos wrt

runHandler :: InputState -> Refresher -> Event -> RepeatCount -> Bool -> EventHandler -> IO (Maybe Cmd)
runHandler is wrt evt n positive = \case
  Simple cmd -> pure (Just cmd)
  Conditional handler -> eventContext is wrt >>= handler evt n positive

-- | Application customized binding.
customBinding :: InputState -> Refresher -> Event -> RepeatCount -> Bool -> IO (Maybe Cmd)
customBinding is wrt evt n positive = do
  bindings <- readIORef (isCustomBindings is)
  case lookupBinding evt bindings of
    Just handler -> runHandler is wrt evt n positive handler
    Nothing -> case lookupBinding Any bindings of
      Just handler -> runHandler is wrt evt n positive handler
      Nothing -> pure Nothing

-- | Read more keys while they may complete a bound sequence. Returns the
--   command, if any, and the (possibly extended) event.
customSeqBinding :: InputState -> RawReader -> Refresher -> Event -> RepeatCount -> Bool -> IO (Maybe Cmd, Event)
customSeqBinding is rdr wrt evt0 n positive = do
  bindings <- readIORef (isCustomBindings is)
  let loop evt
        | not (hasDescendant evt bindings) = pure (Nothing, evt)
        | otherwise = do
            sndKey <- nextKey rdr True
            case evt of
              KeySeq keys -> do
                let evt' = KeySeq (keys ++ [sndKey])
                cmd <- maybe (pure Nothing) (runHandler is wrt evt' n positive) (lookupBinding evt' bindings)
                case cmd of
                  Just _ -> pure (cmd, evt')
                  Nothing -> loop evt'
              _ -> pure (Nothing, evt)
  loop evt0
