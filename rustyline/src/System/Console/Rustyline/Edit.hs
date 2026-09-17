-- | Command processor: the state during line editing and its rendering
--   (@edit.rs@).
module System.Console.Rustyline.Edit
  ( State (..),
    RefreshKind (..),
    newState,
    initState,
    stateContext,
    highlighter,
    stateNextCmd,
    refresher,

    -- * Buffer access
    readLine,
    withLine,
    changesBegin,
    changesEnd,
    changesTruncate,

    -- * Rendering
    backup,
    restore,
    moveCursorS,
    moveCursorToEnd,
    moveCursorAtLeftmostS,
    repaint,
    refreshLineS,
    refreshLineWithMsg,
    refreshPromptAndLine,
    hintS,
    isDefaultPrompt,
    hasHint,
    validateS,
    clearScreenS,

    -- * Edits
    editInsert,
    editReplaceChar,
    editOverwriteChar,
    editYank,
    editYankPop,
    editMoveBackward,
    editMoveForward,
    editMoveHome,
    editMoveEnd,
    editMoveBufferStart,
    editMoveBufferEnd,
    editKill,
    editInsertText,
    editTransposeChars,
    editMoveToPrevWord,
    editMoveToNextWord,
    editMoveLineUp,
    editMoveLineDown,
    editMoveTo,
    editWord,
    editTransposeWords,
    editHistoryNext,
    editHistorySearch,
    editHistory,
    editIndent,
  )
where

import Control.Exception (throwIO, try)
import Control.Monad (unless, void, when)
import Data.IORef
import Data.Maybe (isJust, isNothing)
import Data.Text (Text)
import Data.Text qualified as T
import System.Console.Rustyline.Context
import System.Console.Rustyline.Error
import System.Console.Rustyline.Helper
import System.Console.Rustyline.Highlight
import System.Console.Rustyline.Hint
import System.Console.Rustyline.History
import System.Console.Rustyline.Internal.Text (lenB, takeB)
import System.Console.Rustyline.Internal.Unicode.Grapheme (graphemeCount)
import System.Console.Rustyline.Keymap
import System.Console.Rustyline.KillRing (KillRing)
import System.Console.Rustyline.Layout
import System.Console.Rustyline.LineBuffer hiding (editWord)
import System.Console.Rustyline.LineBuffer qualified as LB
import System.Console.Rustyline.Prompt
import System.Console.Rustyline.Tty
import System.Console.Rustyline.Undo
import System.Console.Rustyline.Validate
import Prelude hiding (Word)

-- | The state during line editing.
data State = State
  { stOut :: Renderer,
    -- | Prompt to display (rl_prompt)
    stPrompt :: Prompt,
    -- | Prompt Unicode/visible width and height
    stPromptSize :: IORef Position,
    -- | Edited line buffer
    stLine :: IORef LineBuffer,
    stLayout :: IORef Layout,
    -- | Current edited line before history browsing
    stSavedLine :: IORef LineBuffer,
    -- | Changes to the line, for undo/redo
    stChanges :: IORef Changeset,
    stHelper :: Maybe Helper,
    -- | Gives the hinter access to the history
    stHistory :: AnyHistory,
    -- | The history index we are currently editing
    stHistoryIndex :: IORef Int,
    -- | Last hint displayed
    stHint :: IORef (Maybe Hint),
    -- | Whether a char has been highlighted
    stHighlightChar :: IORef Bool
  }

data Info
  = NoHint
  | InfoHint
  | Msg (Maybe Text)

data RefreshKind
  = -- | Do not clear old rows and keep the current layout
    RefreshMin
  | -- | Clear old rows + layout
    RefreshAll
  deriving (Eq, Show)

newState :: Renderer -> Prompt -> Maybe Helper -> AnyHistory -> IO State
newState out p helper history = do
  promptSize <- calculatePosition out (promptRaw p) origin
  let growable = canGrowth True (withCapacity maxLine)
  State out p
    <$> newIORef promptSize
    <*> newIORef growable
    <*> newIORef (newLayout (rendererGcm out))
    <*> newIORef growable
    <*> newIORef newChangeset
    <*> pure helper
    <*> pure history
    <*> (newIORef =<< historyLen history)
    <*> newIORef Nothing
    <*> newIORef False

-- | A state with an empty prompt, the given line and cursor, and a "hint"
--   hint (Rust's test-only @init_state@).
initState :: Renderer -> Text -> Int -> Maybe Helper -> AnyHistory -> IO State
initState out text p helper history =
  State out (prompt "")
    <$> newIORef origin
    <*> newIORef (initLineBuffer text p)
    <*> newIORef (newLayout Unicode)
    <*> newIORef (withCapacity 100)
    <*> newIORef newChangeset
    <*> pure helper
    <*> pure history
    <*> (newIORef =<< historyLen history)
    <*> newIORef (Just (hintText "hint"))
    <*> newIORef False

stateContext :: State -> IO Context
stateContext st = Context (stHistory st) <$> readIORef (stHistoryIndex st)

highlighter :: State -> Maybe Highlighter
highlighter st
  | colorsEnabled (stOut st) = helperHighlighter <$> stHelper st
  | otherwise = Nothing

readLine :: State -> IO LineBuffer
readLine = readIORef . stLine

-- | Run an edit on the line, recording it in the change set.
withLine :: State -> LB Changeset a -> IO a
withLine st m = do
  lb <- readIORef (stLine st)
  cs <- readIORef (stChanges st)
  let (a, lb', cs') = runLB cs lb m
  writeIORef (stLine st) lb'
  writeIORef (stChanges st) cs'
  pure a

changesBegin :: State -> IO Int
changesBegin st = atomicModifyIORef' (stChanges st) (swap . begin)

changesEnd :: State -> IO Bool
changesEnd st = atomicModifyIORef' (stChanges st) (swap . end)

changesTruncate :: State -> Int -> IO ()
changesTruncate st mark = modifyIORef' (stChanges st) (truncateChanges mark)

swap :: (a, b) -> (b, a)
swap (a, b) = (b, a)

-- | Read the next command, handling signals.
stateNextCmd :: State -> InputState -> RawReader -> Bool -> Bool -> IO Cmd
stateNextCmd st is rdr singleEscAbort ignoreExternalPrint = loop
  where
    loop = do
      rc <- try (nextCmd is rdr (refresher st) singleEscAbort ignoreExternalPrint)
      case rc of
        Left (ReadlineSignal SigInterrupt) -> pure Interrupt
        Left (ReadlineSignal SigResize) -> do
          let out = stOut st
          oldCols <- getColumns out
          updateSize out
          newCols <- getColumns out
          layout <- readIORef (stLayout st)
          when (newCols /= oldCols && (posRow (layoutEnd layout) > 0 || posCol (layoutEnd layout) >= newCols)) $ do
            writeIORef (stPromptSize st) =<< calculatePosition out (promptRaw (stPrompt st)) origin
            refreshLineS st
          loop
        Left e -> throwIO e
        Right cmd@(Replace _ _) -> cmd <$ changesBegin st
        Right cmd -> pure cmd

backup :: State -> IO ()
backup st = do
  lb <- readLine st
  modifyIORef' (stSavedLine st) $ \saved ->
    snd (evalLB NoListener saved (update (asStr lb) (pos lb)))

restore :: State -> IO ()
restore st = do
  saved <- readIORef (stSavedLine st)
  withLine st (update (asStr saved) (pos saved))

moveCursorS :: State -> CmdKind -> IO ()
moveCursorS st kind = do
  lb <- readLine st
  promptSize <- readIORef (stPromptSize st)
  -- calculate the desired position of the cursor
  cursor <- calculatePosition (stOut st) (takeB (pos lb) (asStr lb)) promptSize
  layout <- readIORef (stLayout st)
  unless (layoutCursor layout == cursor) $ do
    highlighted <- highlightCharS st kind
    if highlighted
      then refresh st (stPrompt st) promptSize True RefreshAll NoHint
      else do
        moveCursor (stOut st) (layoutCursor layout) cursor
        writeIORef (stLayout st) layout {layoutPromptSize = promptSize, layoutCursor = cursor}

moveCursorToEnd :: State -> IO ()
moveCursorToEnd st = do
  layout <- readIORef (stLayout st)
  unless (layoutCursor layout == layoutEnd layout) $ do
    moveCursor (stOut st) (layoutCursor layout) (layoutEnd layout)
    writeIORef (stLayout st) layout {layoutCursor = layoutEnd layout}

moveCursorAtLeftmostS :: State -> RawReader -> IO ()
moveCursorAtLeftmostS st = moveCursorAtLeftmost (stOut st)

repaint :: State -> RefreshKind -> IO ()
repaint st kind = do
  promptSize <- readIORef (stPromptSize st)
  refresh st (stPrompt st) promptSize True kind InfoHint

refresh :: State -> Prompt -> Position -> Bool -> RefreshKind -> Info -> IO ()
refresh st p promptSize defaultPrompt kind info = do
  info' <- case info of
    NoHint -> pure Nothing
    InfoHint -> fmap hintDisplay <$> readIORef (stHint st)
    Msg msg -> pure msg
  lb <- readLine st
  layout <- readIORef (stLayout st)
  let out = stOut st
  case kind of
    RefreshMin -> refreshLine out p lb info' Nothing layout (highlighter st)
    RefreshAll -> do
      newLayout' <- computeLayout out promptSize defaultPrompt lb info'
      refreshLine out p lb info' (Just layout) newLayout' (highlighter st)
      writeIORef (stLayout st) newLayout'

hintS :: State -> IO ()
hintS st = case stHelper st of
  Just helper -> do
    lb <- readLine st
    ctx <- stateContext st
    h <- helperHinter helper (asStr lb) (pos lb) ctx
    writeIORef (stHint st) $ case h of
      Just val | not (T.null (hintDisplay val)) -> Just val
      _ -> Nothing
  Nothing -> writeIORef (stHint st) Nothing

highlightCharS :: State -> CmdKind -> IO Bool
highlightCharS st kind = case highlighter st of
  Nothing -> pure False
  Just hl -> do
    lb <- readLine st
    highlighted <- highlightChar hl (asStr lb) (pos lb) kind
    previously <- readIORef (stHighlightChar st)
    if highlighted
      then True <$ writeIORef (stHighlightChar st) True
      else
        if previously
          then -- previously highlighted => force a full refresh
            True <$ writeIORef (stHighlightChar st) False
          else pure False

isDefaultPrompt :: State -> IO Bool
isDefaultPrompt st = layoutDefaultPrompt <$> readIORef (stLayout st)

hasHint :: State -> IO Bool
hasHint st = isJust <$> readIORef (stHint st)

validateS :: State -> IO ValidationResult
validateS st = case stHelper st of
  Nothing -> pure (Valid Nothing)
  Just helper -> do
    _ <- changesBegin st
    lb <- readLine st
    result <- validate (helperValidator helper) (ValidationContext (asStr lb))
    corrected <- changesEnd st
    hinted <- hasHint st
    case result of
      Incomplete -> pure ()
      -- Accept the line regardless of where the cursor is.
      Valid msg ->
        when (corrected || hinted || isJust msg) $
          -- Force a refresh without hints to leave the previous line as the
          -- user typed it after a newline.
          refreshLineWithMsg st msg ForcedRefresh
      Invalid msg ->
        when (corrected || hinted || isJust msg) $
          refreshLineWithMsg st msg OtherKind
    pure result

refreshLineS :: State -> IO ()
refreshLineS st = do
  hintS st
  _ <- highlightCharS st OtherKind
  repaint st RefreshAll

refreshLineWithMsg :: State -> Maybe Text -> CmdKind -> IO ()
refreshLineWithMsg st msg kind = do
  writeIORef (stHint st) Nothing
  _ <- highlightCharS st kind
  promptSize <- readIORef (stPromptSize st)
  refresh st (stPrompt st) promptSize True RefreshAll (Msg msg)

refreshPromptAndLine :: State -> Text -> IO ()
refreshPromptAndLine st p = do
  promptSize <- calculatePosition (stOut st) p origin
  hintS st
  _ <- highlightCharS st OtherKind
  refresh st (prompt p) promptSize False RefreshAll InfoHint

externalPrintS :: State -> Text -> IO ()
externalPrintS st msg = do
  let out = stOut st
  beginSynchronizedUpdate out
  clearRows out =<< readIORef (stLayout st)
  writeAndFlush out msg
  unless ("\n" `T.isSuffixOf` msg) $ writeAndFlush out "\n"
  repaint st RefreshMin
  endSynchronizedUpdate out

-- | The keymap's view of the state.
refresher :: State -> Refresher
refresher st =
  Refresher
    { rfRefreshLine = refreshLineS st,
      rfRefreshLineWithMsg = refreshLineWithMsg st,
      rfRefreshPromptAndLine = refreshPromptAndLine st,
      rfDoingInsert = void (changesBegin st),
      rfDoneInserting = void (changesEnd st),
      rfLastInsert = lastInsert <$> readIORef (stChanges st),
      rfIsCursorAtEnd = isCursorAtEnd <$> readLine st,
      rfHasHint = hasHint st,
      rfHintText = (>>= hintCompletion) <$> readIORef (stHint st),
      rfLine = asStr <$> readLine st,
      rfPos = pos <$> readLine st,
      rfExternalPrint = externalPrintS st
    }

clearScreenS :: State -> IO ()
clearScreenS st = do
  clearScreen (stOut st)
  modifyIORef' (stLayout st) (\l -> l {layoutCursor = origin, layoutEnd = origin})

-- | Insert the character at the cursor position.
editInsert :: State -> Char -> RepeatCount -> IO ()
editInsert st ch n =
  withLine st (insert ch n) >>= \case
    Nothing -> pure ()
    Just False -> refreshLineS st
    Just True -> do
      noPreviousMsg <- not . layoutHasInfo <$> readIORef (stLayout st)
      hintS st
      highlighted <- highlightCharS st OtherKind
      let width = cwidh ch
      cols <- getColumns (stOut st)
      layout <- readIORef (stLayout st)
      hint <- readIORef (stHint st)
      if n == 1
        && width /= 0 -- Ctrl-V + \t or \n ...
        && posCol (layoutCursor layout) + width < cols
        && (isNothing hint && noPreviousMsg) -- TODO refresh only current line
        && not highlighted
        then do
          -- Avoid a full update of the line in the trivial case.
          let shift p = p {posCol = posCol p + width}
          writeIORef (stLayout st) layout {layoutCursor = shift (layoutCursor layout), layoutEnd = shift (layoutEnd layout)}
          writeAndFlush (stOut st) (T.singleton ch)
        else repaint st RefreshAll

-- | Replace a single (or n) character(s) under the cursor (Vi mode).
editReplaceChar :: State -> Char -> RepeatCount -> IO ()
editReplaceChar st ch n = do
  _ <- changesBegin st
  succeed <-
    withLine st (delete n) >>= \case
      Just chars -> do
        _ <- withLine st (insert ch (graphemeCount chars))
        _ <- withLine st (moveBackward 1)
        pure True
      Nothing -> pure False
  _ <- changesEnd st
  when succeed (refreshLineS st)

-- | Overwrite the character under the cursor (Vi mode).
editOverwriteChar :: State -> Char -> IO ()
editOverwriteChar st ch = do
  lb <- readLine st
  case nextPos 1 lb of
    Just e -> do
      withLine st (replace (pos lb) e (T.singleton ch))
      refreshLineS st
    Nothing -> pure ()

-- | Yank/paste the text at the current position.
editYank :: State -> InputState -> Text -> Anchor -> RepeatCount -> IO ()
editYank st is text anchor n = do
  when (anchor == After) $ void (withLine st (moveForward 1))
  withLine st (yank text n) >>= \case
    Just _ -> do
      vi <- isViCmdMode is
      when vi $ void (withLine st (moveBackward 1))
      refreshLineS st
    Nothing -> pure ()

-- | Delete previously yanked text and yank/paste the text at the current
--   position.
editYankPop :: State -> Int -> Text -> IO ()
editYankPop st yankSize text = do
  _ <- changesBegin st
  r <- withLine st (yankPop yankSize text)
  when (isJust r) (refreshLineS st)
  void (changesEnd st)

editMove :: State -> CmdKind -> LB Changeset Bool -> IO ()
editMove st kind m = do
  ok <- withLine st m
  when ok (moveCursorS st kind)

editMoveBackward, editMoveForward :: State -> RepeatCount -> IO ()
editMoveBackward st n = editMove st MoveCursor (moveBackward n)
editMoveForward st n = editMove st MoveCursor (moveForward n)

editMoveHome, editMoveEnd, editMoveBufferStart :: State -> IO ()
editMoveHome st = editMove st MoveCursor moveHome
editMoveEnd st = editMove st MoveCursor moveEnd
editMoveBufferStart st = editMove st MoveCursor moveBufferStart

editMoveBufferEnd :: State -> CmdKind -> IO ()
editMoveBufferEnd st kind = editMove st kind moveBufferEnd

-- | Listener for 'editKill': records the change, feeds the kill ring and
--   works out whether a partial screen update is enough.
data KillProxy = KillProxy
  { kpChanges :: Changeset,
    kpKillRing :: KillRing,
    -- | current layout (before kill)
    kpLayout :: Layout,
    -- | current cursor (byte) position (before kill)
    kpPos :: Int,
    -- | end (before kill)
    kpEnd :: Int,
    -- | cursor shift (columns) after kill
    kpCursorShift :: Unit,
    -- | end of line shift (columns) after kill
    kpEndShift :: Unit,
    -- | whether a partial screen update can be done
    kpTrivial :: Bool
  }

instance DeleteListener KillProxy where
  startKilling p = p {kpKillRing = startKilling (kpKillRing p)}
  onDelete idx string dir p0
    | not (kpTrivial p) = p
    | dir == DirBackward =
        if kpPos p == kpEnd p
          then -- backward from eol
            let width = layoutWidth (kpLayout p) string
             in p {kpCursorShift = kpCursorShift p + width, kpEndShift = kpEndShift p + width}
          else p {kpTrivial = False}
    | idx == kpPos p && kpPos p + lenB string == kpEnd p =
        -- forward to eol
        p {kpEndShift = kpEndShift p + layoutWidth (kpLayout p) string}
    | otherwise = p {kpTrivial = False}
    where
      p =
        p0
          { kpChanges = deleteChange idx string (kpChanges p0),
            kpKillRing = onDelete idx string dir (kpKillRing p0)
          }
  stopKilling p = p {kpKillRing = stopKilling (kpKillRing p)}

editKill :: State -> Movement -> IORef KillRing -> IO ()
editKill st mvt killRingRef = do
  lb <- readLine st
  layout0 <- readIORef (stLayout st)
  cs <- readIORef (stChanges st)
  kr <- readIORef killRingRef
  let proxy0 =
        KillProxy
          { kpChanges = cs,
            kpKillRing = kr,
            kpLayout = layout0,
            kpPos = pos lb,
            kpEnd = len lb,
            kpCursorShift = 0,
            kpEndShift = 0,
            kpTrivial = posRow (layoutCursor layout0) == posRow (layoutEnd layout0)
          }
      (killed, lb', proxy) = runLB proxy0 lb (kill mvt)
  writeIORef (stLine st) lb'
  writeIORef (stChanges st) (kpChanges proxy)
  writeIORef killRingRef (kpKillRing proxy)
  when killed $ do
    let cursorShift = kpCursorShift proxy
        endShift = kpEndShift proxy
        noPreviousMsg = not (layoutHasInfo layout0)
    hintS st
    highlighted <- highlightCharS st OtherKind
    hint <- readIORef (stHint st)
    layout <- readIORef (stLayout st)
    if kpTrivial proxy
      && cursorShift <= posCol (layoutCursor layout)
      && endShift <= posCol (layoutEnd layout)
      && (isNothing hint && noPreviousMsg)
      && not highlighted
      then do
        -- Avoid a full update of the line in the trivial case.
        let old = layoutCursor layout
            cursor = old {posCol = posCol old - cursorShift}
            endP = (layoutEnd layout) {posCol = posCol (layoutEnd layout) - endShift}
        when (cursorShift /= 0) $ moveCursor (stOut st) old cursor
        writeIORef (stLayout st) layout {layoutCursor = cursor, layoutEnd = endP}
        clearToEol (stOut st)
      else repaint st RefreshAll

editInsertText :: State -> Text -> IO ()
editInsertText st text = unless (T.null text) $ do
  cursor <- pos <$> readLine st
  _ <- withLine st (insertStr cursor text)
  refreshLineS st

-- | Run a grouped edit and refresh when it succeeded.
groupedEdit :: State -> LB Changeset Bool -> IO ()
groupedEdit st m = do
  _ <- changesBegin st
  ok <- withLine st m
  _ <- changesEnd st
  when ok (refreshLineS st)

-- | Exchange the char before the cursor with the character at the cursor.
editTransposeChars :: State -> IO ()
editTransposeChars st = groupedEdit st transposeChars

editMoveToPrevWord :: State -> Word -> RepeatCount -> IO ()
editMoveToPrevWord st wordDef n = editMove st MoveCursor (moveToPrevWord wordDef n)

editMoveToNextWord :: State -> At -> Word -> RepeatCount -> IO ()
editMoveToNextWord st at wordDef n = editMove st MoveCursor (moveToNextWord at wordDef n)

-- | Move the cursor to the same column in the line above.
editMoveLineUp :: State -> RepeatCount -> IO Bool
editMoveLineUp st n = do
  layout <- readIORef (stLayout st)
  ok <- withLine st (moveToLineUp n layout)
  ok <$ when ok (moveCursorS st MoveCursor)

-- | Move the cursor to the same column in the line below.
editMoveLineDown :: State -> RepeatCount -> IO Bool
editMoveLineDown st n = do
  layout <- readIORef (stLayout st)
  ok <- withLine st (moveToLineDown n layout)
  ok <$ when ok (moveCursorS st MoveCursor)

editMoveTo :: State -> CharSearch -> RepeatCount -> IO ()
editMoveTo st cs n = editMove st MoveCursor (moveTo cs n)

editWord :: State -> WordAction -> IO ()
editWord st a = groupedEdit st (LB.editWord a)

editTransposeWords :: State -> RepeatCount -> IO ()
editTransposeWords st n = groupedEdit st (transposeWords n)

-- | Replace the line with an entry, as one undoable change.
replaceWithEntry :: State -> Text -> Int -> IO ()
replaceWithEntry st entry p = do
  _ <- changesBegin st
  withLine st (update entry p)
  void (changesEnd st)

-- | Substitute the currently edited line with the next or previous history
--   entry.
editHistoryNext :: State -> Bool -> IO ()
editHistoryNext st prev = do
  let history = stHistory st
  size <- historyLen history
  idx <- readIORef (stHistoryIndex st)
  proceed <-
    if
      | size == 0 -> pure False
      | idx == size -> if prev then True <$ backup st else pure False -- save the edited line
      | idx == 0 && prev -> pure False
      | otherwise -> pure True
  when proceed $ do
    (i, dir) <-
      if prev
        then pure (idx - 1, SearchReverse)
        else (idx + 1, SearchForward) <$ writeIORef (stHistoryIndex st) (idx + 1)
    if i < size
      then
        historyGet history i dir >>= \case
          Just r -> do
            writeIORef (stHistoryIndex st) (srIdx r)
            replaceWithEntry st (srEntry r) (lenB (srEntry r))
            refreshLineS st
          Nothing -> pure ()
      else do
        -- Restore current edited line
        restore st
        refreshLineS st

-- | Non-incremental, anchored search.
editHistorySearch :: State -> SearchDirection -> IO ()
editHistorySearch st dir = do
  let history = stHistory st
  size <- historyLen history
  idx <- readIORef (stHistoryIndex st)
  if size == 0 || (idx == size && dir == SearchForward) || (idx == 0 && dir == SearchReverse)
    then beep (stOut st)
    else do
      let idx' = if dir == SearchReverse then idx - 1 else idx + 1
      writeIORef (stHistoryIndex st) idx'
      lb <- readLine st
      historyStartsWith history (takeB (pos lb) (asStr lb)) idx' dir >>= \case
        Just sr -> do
          writeIORef (stHistoryIndex st) (srIdx sr)
          replaceWithEntry st (srEntry sr) (srPos sr)
          refreshLineS st
        Nothing -> beep (stOut st)

-- | Substitute the currently edited line with the first/last history entry.
editHistory :: State -> Bool -> IO ()
editHistory st first = do
  let history = stHistory st
  size <- historyLen history
  idx <- readIORef (stHistoryIndex st)
  proceed <-
    if
      | size == 0 -> pure False
      | idx == size -> if first then True <$ backup st else pure False
      | idx == 0 && first -> pure False
      | otherwise -> pure True
  when proceed $
    if first
      then
        historyGet history 0 SearchForward >>= \case
          Just r -> do
            writeIORef (stHistoryIndex st) (srIdx r)
            replaceWithEntry st (srEntry r) (lenB (srEntry r))
            refreshLineS st
          Nothing -> pure ()
      else do
        writeIORef (stHistoryIndex st) size
        restore st
        refreshLineS st

-- | Change the indentation of the lines covered by the movement.
editIndent :: State -> Movement -> Int -> Bool -> IO ()
editIndent st mvt amount dedent = do
  ok <- withLine st (indent mvt amount dedent)
  when ok (refreshLineS st)
