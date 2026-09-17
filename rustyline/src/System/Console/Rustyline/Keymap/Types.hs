-- | The command vocabulary shared by keymaps, bindings and the editor
--   (the data half of @keymap.rs@).
--
--   A few constructors are renamed relative to Rust to avoid clashes in one
--   Haskell namespace: 'Word' is 'WordBig' \/ 'WordEmacs' \/ 'WordVi',
--   'CharSearch' uses a @CS@ prefix and 'InputMode' a @Mode@ suffix.
module System.Console.Rustyline.Keymap.Types
  ( RepeatCount,
    repeatCountMax,
    Cmd (..),
    shouldResetKillRing,
    isRepeatableChange,
    isRepeatable,
    redoCmd,
    Word (..),
    At (..),
    Anchor (..),
    CharSearch (..),
    oppositeCharSearch,
    Movement (..),
    redoMovement,
    InputMode (..),
  )
where

import Data.Text (Text)
import System.Console.Rustyline.Internal.Text (lenB)
import Prelude hiding (Word)

-- | The number of times one command should be repeated.
type RepeatCount = Int

-- | @RepeatCount::MAX@ (Rust uses @u16@).
repeatCountMax :: RepeatCount
repeatCountMax = 65535

-- | Commands.
data Cmd
  = -- | abort
    Abort
  | -- | accept-line
    AcceptLine
  | -- | beginning-of-history
    BeginningOfHistory
  | -- | capitalize-word
    CapitalizeWord
  | -- | clear-screen
    ClearScreen
  | -- | complete
    Complete
  | -- | complete-backward
    CompleteBackward
  | -- | complete-hint
    CompleteHint
  | -- | Dedent current line
    Dedent Movement
  | -- | downcase-word
    DowncaseWord
  | -- | vi-eof-maybe
    EndOfFile
  | -- | end-of-history
    EndOfHistory
  | -- | forward-search-history (incremental search)
    ForwardSearchHistory
  | -- | history-search-backward (common prefix search)
    HistorySearchBackward
  | -- | history-search-forward (common prefix search)
    HistorySearchForward
  | -- | Indent current line
    Indent Movement
  | -- | Insert text
    Insert RepeatCount Text
  | -- | Interrupt signal (Ctrl-C)
    Interrupt
  | -- | backward-delete-char, backward-kill-line, backward-kill-word,
    --   delete-char, kill-line, kill-word, unix-line-discard,
    --   unix-word-rubout, vi-delete, vi-delete-to, vi-rubout
    Kill Movement
  | -- | backward-char, backward-word, beginning-of-line, end-of-line,
    --   forward-char, forward-word, vi-char-search, vi-end-word,
    --   vi-next-word, vi-prev-word
    Move Movement
  | -- | next-history
    NextHistory
  | -- | No action
    Noop
  | -- | repaint
    Repaint
  | -- | vi-replace
    Overwrite Char
  | -- | previous-history
    PreviousHistory
  | -- | quoted-insert
    QuotedInsert
  | -- | vi-change-char
    ReplaceChar RepeatCount Char
  | -- | vi-change-to, vi-substitute
    Replace Movement (Maybe Text)
  | -- | reverse-search-history (incremental search)
    ReverseSearchHistory
  | -- | self-insert
    SelfInsert RepeatCount Char
  | -- | Suspend signal (Ctrl-Z on unix platform)
    Suspend
  | -- | transpose-chars
    TransposeChars
  | -- | transpose-words
    TransposeWords RepeatCount
  | -- | undo
    Undo RepeatCount
  | -- | Unsupported / unexpected
    Unknown
  | -- | upcase-word
    UpcaseWord
  | -- | vi-yank-to
    ViYankTo Movement
  | -- | yank, vi-put
    Yank RepeatCount Anchor
  | -- | yank-pop
    YankPop
  | -- | Move the cursor to the line above, or switch to the previous
    --   history entry if the cursor is already on the first line.
    LineUpOrPreviousHistory RepeatCount
  | -- | Move the cursor to the line below, or switch to the next history
    --   entry if the cursor is already on the last line.
    LineDownOrNextHistory RepeatCount
  | -- | Inserts a newline
    Newline
  | -- | Either accepts or inserts a newline. Always inserts a newline if the
    --   input is not valid. The flag (@accept_in_the_middle@) says whether
    --   valid input is accepted even when the cursor is not at the end.
    AcceptOrInsertLine Bool
  deriving (Eq, Show)

-- | Whether the command should reset the kill ring.
shouldResetKillRing :: Cmd -> Bool
shouldResetKillRing = \case
  Kill (BackwardChar _) -> True
  Kill (ForwardChar _) -> True
  ClearScreen -> False
  Kill _ -> False
  Replace _ _ -> False
  Noop -> False
  Suspend -> False
  Yank _ _ -> False
  YankPop -> False
  _ -> True

isRepeatableChange :: Cmd -> Bool
isRepeatableChange = \case
  Dedent _ -> True
  Indent _ -> True
  Insert _ _ -> True
  Kill _ -> True
  ReplaceChar _ _ -> True
  Replace _ _ -> True
  SelfInsert _ _ -> True
  ViYankTo _ -> True
  Yank _ _ -> True
  _ -> False

isRepeatable :: Cmd -> Bool
isRepeatable = \case
  Move _ -> True
  c -> isRepeatableChange c

repeatCount :: RepeatCount -> Maybe RepeatCount -> RepeatCount
repeatCount previous new = maybe previous id new

-- | Replay this command with a possibly different 'RepeatCount'. The second
--   argument is the text of the last insertion (@Refresher::last_insert@).
--   Only valid for repeatable commands.
redoCmd :: Cmd -> Maybe RepeatCount -> Maybe Text -> Cmd
redoCmd cmd new lastInsert = case cmd of
  Dedent mvt -> Dedent (redoMovement mvt new)
  Indent mvt -> Indent (redoMovement mvt new)
  Insert previous text -> Insert (repeatCount previous new) text
  Kill mvt -> Kill (redoMovement mvt new)
  Move mvt -> Move (redoMovement mvt new)
  ReplaceChar previous c -> ReplaceChar (repeatCount previous new) c
  Replace mvt Nothing -> case mvt of
    ForwardChar 0 -> Replace (ForwardChar (maybe 0 lenB lastInsert)) lastInsert
    _ -> Replace (redoMovement mvt new) lastInsert
  Replace mvt text -> Replace (redoMovement mvt new) text
  SelfInsert previous c -> case lastInsert of
    -- consecutive char inserts are repeatable, not only the last one
    Just text -> Insert (repeatCount previous new) text
    Nothing -> SelfInsert (repeatCount previous new) c
  ViYankTo mvt -> ViYankTo (redoMovement mvt new)
  Yank previous anchor -> Yank (repeatCount previous new) anchor
  _ -> error ("redoCmd: not repeatable: " <> show cmd)

-- | Different word definitions.
data Word
  = -- | non-blanks characters
    WordBig
  | -- | alphanumeric characters
    WordEmacs
  | -- | alphanumeric (and '_') characters
    WordVi
  deriving (Eq, Show)

-- | Where to move with respect to word boundary.
data At
  = -- | Start of word.
    Start
  | -- | Before end of word.
    BeforeEnd
  | -- | After end of word.
    AfterEnd
  deriving (Eq, Show)

-- | Where to paste (relative to cursor position).
data Anchor
  = -- | After cursor
    After
  | -- | Before cursor
    Before
  deriving (Eq, Show)

-- | Character search.
data CharSearch
  = -- | Forward search
    CSForward Char
  | -- | Forward search until
    CSForwardBefore Char
  | -- | Backward search
    CSBackward Char
  | -- | Backward search until
    CSBackwardAfter Char
  deriving (Eq, Show)

oppositeCharSearch :: CharSearch -> CharSearch
oppositeCharSearch = \case
  CSForward c -> CSBackward c
  CSForwardBefore c -> CSBackwardAfter c
  CSBackward c -> CSForward c
  CSBackwardAfter c -> CSForwardBefore c

-- | Where to move.
data Movement
  = -- | Whole current line (not really a movement but a range)
    WholeLine
  | -- | beginning-of-line
    BeginningOfLine
  | -- | end-of-line
    EndOfLine
  | -- | backward-word, vi-prev-word (backward until start of word)
    BackwardWord RepeatCount Word
  | -- | forward-word, vi-end-word, vi-next-word (forward until start/end of
    --   word)
    ForwardWord RepeatCount At Word
  | -- | character-search, character-search-backward, vi-char-search
    ViCharSearch RepeatCount CharSearch
  | -- | vi-first-print
    ViFirstPrint
  | -- | backward-char
    BackwardChar RepeatCount
  | -- | forward-char
    ForwardChar RepeatCount
  | -- | move to the same column on the previous line
    LineUp RepeatCount
  | -- | move to the same column on the next line
    LineDown RepeatCount
  | -- | Whole user input (not really a movement but a range)
    WholeBuffer
  | -- | beginning-of-buffer
    BeginningOfBuffer
  | -- | end-of-buffer
    EndOfBuffer
  deriving (Eq, Show)

-- | Replay this movement with a possibly different 'RepeatCount'.
redoMovement :: Movement -> Maybe RepeatCount -> Movement
redoMovement mvt new = case mvt of
  BackwardWord previous word -> BackwardWord (repeatCount previous new) word
  ForwardWord previous at word -> ForwardWord (repeatCount previous new) at word
  ViCharSearch previous cs -> ViCharSearch (repeatCount previous new) cs
  BackwardChar previous -> BackwardChar (repeatCount previous new)
  ForwardChar previous -> ForwardChar (repeatCount previous new)
  LineUp previous -> LineUp (repeatCount previous new)
  LineDown previous -> LineDown (repeatCount previous new)
  other -> other

-- | Vi input modes.
data InputMode
  = -- | Vi Command/Alternate
    CommandMode
  | -- | Insert/Input mode
    InsertMode
  | -- | Overwrite mode
    ReplaceMode
  deriving (Eq, Show)
