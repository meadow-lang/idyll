-- | Execution of the commands that only need the editor state
--   (@command.rs@).
module System.Console.Rustyline.Command
  ( Status (..),
    execute,
    completeHintLine,
  )
where

import Control.Exception (throwIO)
import Control.Monad (void, when)
import Data.IORef
import Data.Text qualified as T
import System.Console.Rustyline.Config (Config (..))
import System.Console.Rustyline.Edit
import System.Console.Rustyline.Error
import System.Console.Rustyline.Highlight (CmdKind (..))
import System.Console.Rustyline.Hint (Hint (..))
import System.Console.Rustyline.History (SearchDirection (..))
import System.Console.Rustyline.Internal.Unicode.CharProps (isWhitespace)
import System.Console.Rustyline.Keymap
import System.Console.Rustyline.KillRing
import System.Console.Rustyline.LineBuffer hiding (editWord)
import System.Console.Rustyline.Tty (Renderer (..))
import System.Console.Rustyline.Undo (undo)
import System.Console.Rustyline.Validate
import Prelude hiding (Word)

data Status
  = Proceed
  | Submit
  deriving (Eq, Show)

isAccept :: Cmd -> Bool
isAccept = \case
  EndOfFile -> True
  AcceptLine -> True
  AcceptOrInsertLine _ -> True
  Newline -> True
  _ -> False

execute :: Cmd -> State -> InputState -> IORef KillRing -> Config -> IO Status
execute cmd s is killRing config = do
  when (isAccept cmd) $ do
    hinted <- hasHint s
    defaultPrompt <- isDefaultPrompt s
    highlighted <- readIORef (stHighlightChar s)
    when (hinted || not defaultPrompt || highlighted) $
      -- Force a refresh without hints to leave the previous line as the user
      -- typed it after a newline.
      refreshLineWithMsg s Nothing ForcedRefresh
  let proceed m = Proceed <$ m
  case cmd of
    CompleteHint -> proceed (completeHintLine s)
    SelfInsert n c -> proceed (editInsert s c n)
    Insert n text -> proceed (editYank s is text Before n)
    -- Move to the beginning of line.
    Move BeginningOfLine -> proceed (editMoveHome s)
    Move ViFirstPrint -> proceed $ do
      editMoveHome s
      lb <- readLine s
      when (maybe False (isWhitespace . fst) (T.uncons (asStr lb))) $
        editMoveToNextWord s Start WordBig 1
    -- Move back a character.
    Move (BackwardChar n) -> proceed (editMoveBackward s n)
    ReplaceChar n c -> proceed (editReplaceChar s c n)
    Replace mvt text -> proceed $ do
      editKill s mvt killRing
      mapM_ (editInsertText s) text
    Overwrite c -> proceed (editOverwriteChar s c)
    EndOfFile -> do
      lb <- readLine s
      if
        | isEmpty lb -> throwIO Eof
        | not (isEmacsMode is) -> pure Submit
        | otherwise -> pure Proceed
    -- Move to the end of line.
    Move EndOfLine -> proceed (editMoveEnd s)
    -- Move forward a character.
    Move (ForwardChar n) -> proceed (editMoveForward s n)
    -- Clear the screen leaving the current line at the top of the screen.
    ClearScreen -> proceed (clearScreenS s >> refreshLineS s)
    -- Fetch the next command from the history list.
    NextHistory -> proceed (editHistoryNext s False)
    -- Fetch the previous command from the history list.
    PreviousHistory -> proceed (editHistoryNext s True)
    LineUpOrPreviousHistory n -> proceed $ do
      moved <- editMoveLineUp s n
      when (not moved) $ editHistoryNext s True
    LineDownOrNextHistory n -> proceed $ do
      moved <- editMoveLineDown s n
      when (not moved) $ editHistoryNext s False
    HistorySearchBackward -> proceed (editHistorySearch s SearchReverse)
    HistorySearchForward -> proceed (editHistorySearch s SearchForward)
    -- Exchange the char before cursor with the character at cursor.
    TransposeChars -> proceed (editTransposeChars s)
    -- retrieve (yank) last item killed
    Yank n anchor -> proceed $ do
      text <- atomicModifyIORef' killRing (swap . yankText)
      mapM_ (\t -> editYank s is t anchor n) text
    ViYankTo mvt -> proceed $ do
      lb <- readLine s
      mapM_ (\text -> modifyIORef' killRing (killText text Append)) (copy mvt lb)
    Newline -> proceed (editInsert s '\n' 1)
    Repaint -> proceed (refreshLineS s)
    AcceptLine -> acceptOrInsert
    AcceptOrInsertLine _ -> acceptOrInsert
    -- move to first entry in history
    BeginningOfHistory -> proceed (editHistory s True)
    -- move to last entry in history
    EndOfHistory -> proceed (editHistory s False)
    -- move backwards one word
    Move (BackwardWord n wordDef) -> proceed (editMoveToPrevWord s wordDef n)
    -- capitalize word after point
    CapitalizeWord -> proceed (editWord s Capitalize)
    Kill mvt -> proceed (editKill s mvt killRing)
    -- move forwards one word
    Move (ForwardWord n at wordDef) -> proceed (editMoveToNextWord s at wordDef n)
    Move (LineUp n) -> proceed (void (editMoveLineUp s n))
    Move (LineDown n) -> proceed (void (editMoveLineDown s n))
    -- Move to the start of the buffer.
    Move BeginningOfBuffer -> proceed (editMoveBufferStart s)
    -- Move to the end of the buffer.
    Move EndOfBuffer -> proceed (editMoveBufferEnd s MoveCursor)
    -- lowercase word after point
    DowncaseWord -> proceed (editWord s Lowercase)
    TransposeWords n -> proceed (editTransposeWords s n)
    -- uppercase word after point
    UpcaseWord -> proceed (editWord s Uppercase)
    YankPop -> proceed $ do
      popped <- atomicModifyIORef' killRing (swap . yankPopText)
      mapM_ (uncurry (editYankPop s)) popped
    Move (ViCharSearch n cs) -> proceed (editMoveTo s cs n)
    Undo n -> proceed $ do
      lb <- readLine s
      cs <- readIORef (stChanges s)
      let (undone, cs', lb') = undo n cs lb
      writeIORef (stChanges s) cs'
      writeIORef (stLine s) lb'
      when undone (refreshLineS s)
    Dedent mvt -> proceed (editIndent s mvt (fromIntegral (indentSize config)) True)
    Indent mvt -> proceed (editIndent s mvt (fromIntegral (indentSize config)) False)
    Interrupt -> do
      -- Move to end, in case cursor was in the middle of the line, so that
      -- next thing application prints goes after the input
      moveCursorToEnd s
      throwIO Interrupted
    -- Ignore the character typed.
    _ -> pure Proceed
  where
    acceptOrInsert = do
      result <- validateS s
      let valid = isValid result
      atEnd <- isEndOfInput <$> readLine s
      case cmd of
        _
          | cmd == AcceptLine -> pure Submit
          | valid && atEnd -> pure Submit
          | AcceptOrInsertLine True <- cmd, valid -> pure Submit
          | otherwise -> do
              when (valid || not (hasMessage result)) $ editInsert s '\n' 1
              pure Proceed

swap :: (a, b) -> (b, a)
swap (a, b) = (b, a)

-- | Complete the current hint.
completeHintLine :: State -> IO ()
completeHintLine s =
  readIORef (stHint s) >>= \case
    Nothing -> pure ()
    Just hint -> do
      _ <- withLine s moveEnd
      case hintCompletion hint of
        Just text -> do
          r <- withLine s (yank text 1)
          when (r == Nothing) $ beep (stOut s)
        Nothing -> beep (stOut s)
      refreshLineS s
