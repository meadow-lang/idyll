-- | Line buffer with current cursor position (@line_buffer.rs@).
--
--   Positions are UTF-8 byte offsets, as in Rust. Rust's listener callbacks
--   (@&mut impl ChangeListener@) become a listener value threaded through
--   the 'LB' state monad: run an edit with 'runLB' and you get back the new
--   buffer and the listener that observed the change.
module System.Console.Rustyline.LineBuffer
  ( -- * Buffer
    LineBuffer,
    maxLine,
    withCapacity,
    canGrowth,
    initLineBuffer,
    asStr,
    pos,
    isCursorAtEnd,
    len,
    isEmpty,
    isEndOfInput,
    graphemeAtCursor,
    nextPos,
    prevPos,
    copy,

    -- * Listeners
    Direction (..),
    WordAction (..),
    DeleteListener (..),
    ChangeListener (..),
    NoListener (..),

    -- * Editing
    LB,
    runLB,
    evalLB,
    setPos,
    update,
    insert,
    yank,
    yankPop,
    moveBackward,
    moveForward,
    moveBufferStart,
    moveBufferEnd,
    moveHome,
    moveEnd,
    delete,
    backspace,
    killLine,
    killBuffer,
    discardLine,
    discardBuffer,
    transposeChars,
    moveToPrevWord,
    deletePrevWord,
    moveToNextWord,
    moveToLineUp,
    moveToLineDown,
    moveTo,
    deleteWord,
    deleteTo,
    editWord,
    transposeWords,
    replace,
    insertStr,
    deleteRange,
    kill,
    indent,
  )
where

import Control.Monad (forM_, when)
import Control.Monad.State.Strict (State, gets, modify', runState)
import Data.Maybe (fromMaybe, isJust)
import Data.Text (Text)
import Data.Text qualified as T
import System.Console.Rustyline.Internal.Text
import System.Console.Rustyline.Internal.Unicode.CharProps (isAlphanumeric, isWhitespace)
import System.Console.Rustyline.Internal.Unicode.Grapheme (graphemeCount, graphemeIndices, graphemes)
import System.Console.Rustyline.Keymap.Types
import System.Console.Rustyline.Layout (Layout (..), Position (..), layoutWidth)
import Prelude hiding (Word)

-- | Default maximum buffer size for the line read.
maxLine :: Int
maxLine = 4096

indentStr :: Text
indentStr = T.replicate 32 " "

-- | Word's case change.
data WordAction
  = -- | Capitalize word
    Capitalize
  | -- | lowercase word
    Lowercase
  | -- | uppercase word
    Uppercase
  deriving (Eq, Show)

-- | Delete (kill) direction.
data Direction
  = -- | After cursor
    DirForward
  | -- | Before cursor
    DirBackward
  deriving (Eq, Show)

-- | Listener to be notified when some text is deleted.
class DeleteListener l where
  -- | used to make the distinction between simple character(s) deletion and
  --   word(s)/line(s) deletion
  startKilling :: l -> l
  startKilling = id

  -- | @string@ deleted at byte index @idx@
  onDelete :: Int -> Text -> Direction -> l -> l

  -- | used to make the distinction between simple character(s) deletion and
  --   word(s)/line(s) deletion
  stopKilling :: l -> l
  stopKilling = id

-- | Listener to be notified when the line is modified.
class (DeleteListener l) => ChangeListener l where
  -- | character inserted at byte index @idx@
  onInsertChar :: Int -> Char -> l -> l

  -- | @string@ inserted at byte index @idx@
  onInsertStr :: Int -> Text -> l -> l

  -- | @old@ text replaced by @new@ text at byte index @idx@
  onReplace :: Int -> Text -> Text -> l -> l

data NoListener = NoListener

instance DeleteListener NoListener where
  onDelete _ _ _ = id

instance ChangeListener NoListener where
  onInsertChar _ _ = id
  onInsertStr _ _ = id
  onReplace _ _ _ = id

-- | Represent the current input (text and cursor position).
data LineBuffer = LineBuffer
  { lbBuf :: !Text,
    lbPos :: !Int,
    lbCanGrowth :: !Bool,
    lbCapacity :: !Int
  }
  deriving (Eq)

instance Show LineBuffer where
  show lb = "LineBuffer { buf: " <> show (lbBuf lb) <> ", pos: " <> show (lbPos lb) <> " }"

-- | A buffer that may not grow beyond @capacity@ bytes (see 'canGrowth').
withCapacity :: Int -> LineBuffer
withCapacity capacity = LineBuffer T.empty 0 False capacity

-- | Set whether to allow dynamic growth.
canGrowth :: Bool -> LineBuffer -> LineBuffer
canGrowth yes lb = lb {lbCanGrowth = yes}

-- | A buffer holding @line@ with the cursor at @p@ (Rust's test-only
--   @LineBuffer::init@).
initLineBuffer :: Text -> Int -> LineBuffer
initLineBuffer line p =
  snd $ evalLB NoListener (withCapacity maxLine) $ do
    _ <- insertStr 0 line
    setPos p

mustTruncate :: LineBuffer -> Int -> Bool
mustTruncate lb newLen = not (lbCanGrowth lb) && newLen > lbCapacity lb

-- | The entire buffer.
asStr :: LineBuffer -> Text
asStr = lbBuf

-- | Current cursor position (byte position).
pos :: LineBuffer -> Int
pos = lbPos

-- | Check if cursor is at the end of input.
isCursorAtEnd :: LineBuffer -> Bool
isCursorAtEnd lb = lbPos lb == lenB (lbBuf lb)

-- | Length of the buffer, in bytes.
len :: LineBuffer -> Int
len = lenB . lbBuf

isEmpty :: LineBuffer -> Bool
isEmpty = T.null . lbBuf

-- | Is the cursor at the end of input (whitespace after the cursor is
--   discarded)?
isEndOfInput :: LineBuffer -> Bool
isEndOfInput lb = lbPos lb >= lenB (trimEndRust (lbBuf lb))

endOfLine :: LineBuffer -> Int
endOfLine (LineBuffer buf p _ _) = maybe (lenB buf) (+ p) (findB '\n' (dropB p buf))

startOfLine :: LineBuffer -> Int
startOfLine (LineBuffer buf p _ _) = maybe 0 (+ 1) (rfindB '\n' (takeB p buf))

-- | The grapheme at the cursor position, if any.
graphemeAtCursor :: LineBuffer -> Maybe Text
graphemeAtCursor lb
  | isCursorAtEnd lb = Nothing
  | otherwise = case graphemes (dropB (lbPos lb) (lbBuf lb)) of
      (g : _) -> Just g
      [] -> Nothing

lastOf :: [a] -> Maybe a
lastOf [] = Nothing
lastOf xs = Just (last xs)

-- | The position just after the @n@-th grapheme following the cursor.
nextPos :: RepeatCount -> LineBuffer -> Maybe Int
nextPos n lb
  | isCursorAtEnd lb = Nothing
  | otherwise =
      (\(i, s) -> i + lbPos lb + lenB s)
        <$> lastOf (take n (graphemeIndices (dropB (lbPos lb) (lbBuf lb))))

-- | The position of the @n@-th grapheme before the cursor.
prevPos :: RepeatCount -> LineBuffer -> Maybe Int
prevPos n lb
  | lbPos lb == 0 = Nothing
  | otherwise =
      fst <$> lastOf (take n (reverse (graphemeIndices (takeB (lbPos lb) (lbBuf lb)))))

-- ---------------------------------------------------------------------------
-- The editing monad

-- | An edit to a line buffer, observed by a listener.
type LB l = State (LineBuffer, l)

-- | Run an edit, returning its result, the new buffer and the listener.
runLB :: l -> LineBuffer -> LB l a -> (a, LineBuffer, l)
runLB l lb m = let (a, (lb', l')) = runState m (lb, l) in (a, lb', l')

-- | 'runLB' without a listener of interest.
evalLB :: l -> LineBuffer -> LB l a -> (a, LineBuffer)
evalLB l lb m = let (a, lb', _) = runLB l lb m in (a, lb')

self :: LB l LineBuffer
self = gets fst

buffer :: LB l Text
buffer = gets (lbBuf . fst)

cursor :: LB l Int
cursor = gets (lbPos . fst)

putPos :: Int -> LB l ()
putPos p = modify' (\(lb, l) -> (lb {lbPos = p}, l))

putBuf :: Text -> LB l ()
putBuf b = modify' (\(lb, l) -> (lb {lbBuf = b}, l))

notify :: (l -> l) -> LB l ()
notify f = modify' (\(lb, l) -> (lb, f l))

-- | Set cursor position (byte position).
setPos :: Int -> LB l ()
setPos p = do
  b <- buffer
  if p <= lenB b then putPos p else error "setPos: position past the end of the buffer"

-- | Set line content and cursor position.
update :: (ChangeListener l) => Text -> Int -> LB l ()
update newBuf p = do
  when (p > lenB newBuf) $ error "update: position past the end of the text"
  end <- gets (len . fst)
  _ <- drain 0 end DirForward
  lb <- self
  let maxCap = lbCapacity lb
  if mustTruncate lb (lenB newBuf)
    then do
      -- Rust slices at the byte capacity; stay on a character boundary
      _ <- insertStr 0 (takeB (floorCharBoundary maxCap newBuf) newBuf)
      putPos (min maxCap p)
    else do
      _ <- insertStr 0 newBuf
      putPos p

-- | Insert the character @n@ times at the cursor and advance the cursor.
--   'Nothing' when the maximum buffer size has been reached, 'Just True'
--   when the text was appended at the end of the line.
insert :: (ChangeListener l) => Char -> RepeatCount -> LB l (Maybe Bool)
insert ch n = do
  lb <- self
  let shift = charLenUtf8 ch * n
  if mustTruncate lb (len lb + shift)
    then pure Nothing
    else do
      let atEnd = isCursorAtEnd lb
          p = lbPos lb
      if n == 1
        then do
          putBuf (insertAt p (T.singleton ch) (lbBuf lb))
          notify (onInsertChar p ch)
        else do
          _ <- insertStr p (T.replicate n (T.singleton ch))
          pure ()
      putPos (p + shift)
      pure (Just atEnd)

insertAt :: Int -> Text -> Text -> Text
insertAt i s b = takeB i b <> s <> dropB i b

-- | Yank/paste @text@ (@n@ times) at the cursor. 'Nothing' when the text is
--   empty or the maximum buffer size has been reached.
yank :: (ChangeListener l) => Text -> RepeatCount -> LB l (Maybe Bool)
yank text n = do
  lb <- self
  let shift = lenB text * n
  if T.null text || mustTruncate lb (len lb + shift)
    then pure Nothing
    else do
      let atEnd = isCursorAtEnd lb
          p = lbPos lb
      _ <- insertStr p (if n == 1 then text else T.replicate n text)
      putPos (p + shift)
      pure (Just atEnd)

-- | Delete previously yanked text and yank @text@ at the cursor.
yankPop :: (ChangeListener l) => Int -> Text -> LB l (Maybe Bool)
yankPop yankSize text = do
  end <- cursor
  let start = end - yankSize
  _ <- drain start end DirForward
  putPos (end - yankSize)
  yank text 1

-- | Move the cursor left.
moveBackward :: RepeatCount -> LB l Bool
moveBackward n = do
  lb <- self
  case prevPos n lb of
    Just p -> putPos p >> pure True
    Nothing -> pure False

-- | Move the cursor right.
moveForward :: RepeatCount -> LB l Bool
moveForward n = do
  lb <- self
  case nextPos n lb of
    Just p -> putPos p >> pure True
    Nothing -> pure False

-- | Move the cursor to the start of the buffer.
moveBufferStart :: LB l Bool
moveBufferStart = do
  p <- cursor
  if p > 0 then putPos 0 >> pure True else pure False

-- | Move the cursor to the end of the buffer.
moveBufferEnd :: LB l Bool
moveBufferEnd = do
  lb <- self
  if isCursorAtEnd lb then pure False else putPos (len lb) >> pure True

-- | Move the cursor to the start of the line.
moveHome :: LB l Bool
moveHome = do
  lb <- self
  let start = startOfLine lb
  if lbPos lb > start then putPos start >> pure True else pure False

-- | Move the cursor to the end of the line.
moveEnd :: LB l Bool
moveEnd = do
  lb <- self
  let end = endOfLine lb
  if lbPos lb == end then pure False else putPos end >> pure True

-- | Delete the grapheme(s) right of the cursor without moving it (the
--   Delete key). Returns the deleted text.
delete :: (DeleteListener l) => RepeatCount -> LB l (Maybe Text)
delete n = do
  lb <- self
  case nextPos n lb of
    Just p -> Just <$> drain (lbPos lb) p DirForward
    Nothing -> pure Nothing

-- | Delete the grapheme(s) left of the cursor (the Backspace key).
backspace :: (DeleteListener l) => RepeatCount -> LB l Bool
backspace n = do
  lb <- self
  case prevPos n lb of
    Just p -> do
      _ <- drain p (lbPos lb) DirBackward
      putPos p
      pure True
    Nothing -> pure False

-- | Kill the text from the cursor to the end of the line.
killLine :: (DeleteListener l) => LB l Bool
killLine = do
  lb <- self
  if not (isEmpty lb) && lbPos lb < len lb
    then do
      let start = lbPos lb
          end = endOfLine lb
      if start == end
        then () <$ delete 1
        else () <$ drain start end DirForward
      pure True
    else pure False

-- | Kill the text from the cursor to the end of the buffer.
killBuffer :: (DeleteListener l) => LB l Bool
killBuffer = do
  lb <- self
  if not (isEmpty lb) && lbPos lb < len lb
    then do
      _ <- drain (lbPos lb) (len lb) DirForward
      pure True
    else pure False

-- | Kill backward from the cursor to the beginning of the line.
discardLine :: (DeleteListener l) => LB l Bool
discardLine = do
  lb <- self
  if lbPos lb > 0 && not (isEmpty lb)
    then do
      let start = startOfLine lb
          end = lbPos lb
      if end == start
        then backspace 1
        else do
          _ <- drain start end DirBackward
          putPos start
          pure True
    else pure False

-- | Kill backward from the cursor to the beginning of the buffer.
discardBuffer :: (DeleteListener l) => LB l Bool
discardBuffer = do
  lb <- self
  if lbPos lb > 0 && not (isEmpty lb)
    then do
      _ <- drain 0 (lbPos lb) DirBackward
      putPos 0
      pure True
    else pure False

-- | Exchange the grapheme before the cursor with the one at the cursor.
transposeChars :: (ChangeListener l) => LB l Bool
transposeChars = do
  lb <- self
  if lbPos lb == 0 || graphemeCount (lbBuf lb) < 2
    then pure False
    else do
      when (isCursorAtEnd lb) $ () <$ moveBackward 1
      chars <- fromMaybe (error "transposeChars: nothing to delete") <$> delete 1
      _ <- moveBackward 1
      _ <- yank chars 1
      _ <- moveForward 1
      pure True

-- | Go left until the start of a word.
prevWordPos :: LineBuffer -> Int -> Word -> RepeatCount -> Maybe Int
prevWordPos lb p wordDef n
  | p == 0 = Nothing
  | otherwise = Just (outer n (reverse (graphemeIndices (takeB p (lbBuf lb)))) 0)
  where
    -- `gis` is the (reversed) grapheme iterator shared across iterations
    outer 0 _ sow = sow
    outer k gis _ = case gis of
      [] -> 0
      (gj : rest) -> inner k gj rest
    inner k (j, y) gis = case gis of
      [] -> 0
      ((xi, x) : rest)
        | isStartOfWord wordDef x y -> outer (k - 1) rest j
        | otherwise -> inner k (xi, x) rest

-- | Move the cursor to the beginning of the previous word.
moveToPrevWord :: Word -> RepeatCount -> LB l Bool
moveToPrevWord wordDef n = do
  lb <- self
  case prevWordPos lb (lbPos lb) wordDef n of
    Just p -> putPos p >> pure True
    Nothing -> pure False

-- | Delete the previous word, keeping the cursor at the start of the
--   current word.
deletePrevWord :: (DeleteListener l) => Word -> RepeatCount -> LB l Bool
deletePrevWord wordDef n = do
  lb <- self
  case prevWordPos lb (lbPos lb) wordDef n of
    Just p -> do
      _ <- drain p (lbPos lb) DirBackward
      putPos p
      pure True
    Nothing -> pure False

nextWordPos :: LineBuffer -> Int -> At -> Word -> RepeatCount -> Maybe Int
nextWordPos lb p at wordDef n
  | p == len lb = Nothing
  | otherwise =
      let gis0 = graphemeIndices (dropB p (lbBuf lb))
          -- BeforeEnd consumes one grapheme up front (the Rust code's
          -- `gi` is then overwritten at the start of the loop)
          gis1 = if at == BeforeEnd then drop 1 gis0 else gis0
          (wp, gi) = outer n gis1 (0, Nothing)
       in if wp == 0
            then
              if wordDef == WordEmacs || at == AfterEnd
                then Just (len lb)
                else case gi of
                  Just (i, _) | i /= 0 -> Just (i + p)
                  _ -> Nothing
            else Just (wp + p)
  where
    -- returns (wp, last value of `gi`)
    outer :: Int -> [(Int, Text)] -> (Int, Maybe (Int, Text)) -> (Int, Maybe (Int, Text))
    outer 0 _ acc = acc
    outer k gis _ = case gis of
      [] -> (0, Nothing)
      (gi : rest) -> inner k gi rest
    inner k gi@(i, x) gis = case gis of
      [] -> (0, Just gi)
      (gj@(j, y) : rest)
        | at == Start && isStartOfWord wordDef x y -> outer (k - 1) rest (j, Just gi)
        | at /= Start && isEndOfWord wordDef x y ->
            let wp = if wordDef == WordEmacs || at == AfterEnd then j else i
             in outer (k - 1) rest (wp, Just gi)
        | otherwise -> inner k gj rest

-- | Move the cursor to the end of the next word.
moveToNextWord :: At -> Word -> RepeatCount -> LB l Bool
moveToNextWord at wordDef n = do
  lb <- self
  case nextWordPos lb (lbPos lb) at wordDef n of
    Just p -> putPos p >> pure True
    Nothing -> pure False

-- | Move the cursor to the same column in the line above.
moveToLineUp :: RepeatCount -> Layout -> LB l Bool
moveToLineUp n layout = do
  LineBuffer buf p _ _ <- self
  case rfindB '\n' (takeB p buf) of
    Nothing -> pure False
    Just off -> do
      let column = layoutWidth layout (sliceB (off + 1) p buf)
          startFor e = maybe 0 (+ 1) (rfindB '\n' (takeB e buf))
          climb :: Int -> Int -> Int -> (Int, Int)
          climb k ds de
            | k <= 1 = (ds, de)
            | ds == 0 = (ds, de)
            | otherwise = let de' = ds - 1 in climb (k - 1) (startFor de') de'
          (destStart, destEnd) = climb n (startFor off) off
          offset = if destStart == 0 then posCol (layoutPromptSize layout) else 0
          gidx = drop (max 0 (column - offset)) (graphemeIndices (sliceB destStart destEnd buf))
      putPos $ case gidx of
        ((idx, _) : _) -> destStart + idx
        [] -> off -- not enough columns
      pure True

-- | @n@ lines up starting from the current one; fails on the first line.
nLinesUp :: LineBuffer -> RepeatCount -> Maybe (Int, Int)
nLinesUp (LineBuffer buf p _ _) n = do
  off <- rfindB '\n' (takeB p buf)
  let end = maybe (lenB buf) (\x -> p + x + 1) (findB '\n' (dropB p buf))
      climb :: Int -> Int -> Int
      climb 0 start = start
      climb k start = case rfindB '\n' (takeB (start - 1) buf) of
        Just o -> climb (k - 1) (o + 1)
        Nothing -> 0
  pure (climb n (off + 1), end)

-- | @n@ lines down starting from the current one; fails on the last line.
nLinesDown :: LineBuffer -> RepeatCount -> Maybe (Int, Int)
nLinesDown (LineBuffer buf p _ _) n = do
  off <- findB '\n' (dropB p buf)
  let start = fromMaybe 0 (rfindB '\n' (takeB p buf))
      descend :: Int -> Int -> Int
      descend 0 end = end
      descend k end = case findB '\n' (dropB end buf) of
        Just o -> descend (k - 1) (end + o + 1)
        Nothing -> lenB buf
  pure (start, descend n (p + off + 1))

-- | Move the cursor to the same column in the line below.
moveToLineDown :: RepeatCount -> Layout -> LB l Bool
moveToLineDown n layout = do
  LineBuffer buf p _ _ <- self
  case findB '\n' (dropB p buf) of
    Nothing -> pure False
    Just off -> do
      let lineStart = maybe 0 (+ 1) (rfindB '\n' (takeB p buf))
          offset = if lineStart == 0 then posCol (layoutPromptSize layout) else 0
          column = layoutWidth layout (sliceB lineStart p buf) + offset
          endFor s = maybe (lenB buf) (+ s) (findB '\n' (dropB s buf))
          descend :: Int -> Int -> Int -> (Int, Int)
          descend k ds de
            | k <= 1 = (ds, de)
            | de == lenB buf = (ds, de)
            | otherwise = let ds' = de + 1 in descend (k - 1) ds' (endFor ds')
          start0 = p + off + 1
          (destStart, destEnd) = descend n start0 (endFor start0)
      putPos $ case drop column (graphemeIndices (sliceB destStart destEnd buf)) of
        ((idx, _) : _) -> destStart + idx
        [] -> destEnd -- not enough columns
      pure True

searchCharPos :: LineBuffer -> CharSearch -> RepeatCount -> Maybe Int
searchCharPos lb@(LineBuffer buf p _ _) cs n = do
  case cs of
    CSBackward c -> backwardHit c
    CSBackwardAfter c -> (+ charLenUtf8 c) <$> backwardHit c
    CSForward c -> do
      (shift, i) <- forwardHit c
      pure (shift + i)
    CSForwardBefore c -> do
      (shift, i) <- forwardHit c
      prev <- lastChar (takeB (shift + i) buf)
      pure (shift + i - charLenUtf8 prev)
  where
    backwardHit c =
      fst <$> lastOf (take n (filter ((== c) . snd) (reverse (charIndices (takeB p buf)))))
    forwardHit c = do
      cc <- graphemeAtCursor lb
      let shift = p + lenB cc
      if shift < lenB buf
        then do
          i <- fst <$> lastOf (take n (filter ((== c) . snd) (charIndices (dropB shift buf))))
          pure (shift, i)
        else Nothing

-- | Move the cursor to the matching character position.
moveTo :: CharSearch -> RepeatCount -> LB l Bool
moveTo cs n = do
  lb <- self
  case searchCharPos lb cs n of
    Just p -> putPos p >> pure True
    Nothing -> pure False

-- | Kill from the cursor to the end of the current word, or, if between
--   words, to the end of the next word.
deleteWord :: (DeleteListener l) => At -> Word -> RepeatCount -> LB l Bool
deleteWord at wordDef n = do
  lb <- self
  case nextWordPos lb (lbPos lb) at wordDef n of
    Just p -> do
      _ <- drain (lbPos lb) p DirForward
      pure True
    Nothing -> pure False

-- | Delete the range specified by a character search.
deleteTo :: (DeleteListener l) => CharSearch -> RepeatCount -> LB l Bool
deleteTo cs n = do
  lb <- self
  let searchResult = case cs of
        CSForwardBefore c -> searchCharPos lb (CSForward c) n
        _ -> searchCharPos lb cs n
  case searchResult of
    Nothing -> pure False
    Just p -> do
      let start = lbPos lb
      case cs of
        CSBackward _ -> backwardDrain p
        CSBackwardAfter _ -> backwardDrain p
        CSForwardBefore _ -> () <$ drain start p DirForward
        CSForward c -> () <$ drain start (p + charLenUtf8 c) DirForward
      pure True
  where
    backwardDrain p = do
      end <- cursor
      putPos p
      () <$ drain p end DirBackward

skipWhitespace :: LineBuffer -> Maybe Int
skipWhitespace lb@(LineBuffer buf p _ _)
  | isCursorAtEnd lb = Nothing
  | otherwise =
      case [i | (i, g) <- graphemeIndices (dropB p buf), T.all isAlphanumeric g] of
        (i : _) -> Just (i + p)
        [] -> Nothing

-- | Alter the next word.
editWord :: (ChangeListener l) => WordAction -> LB l Bool
editWord a = do
  lb <- self
  case skipWhitespace lb of
    Nothing -> pure False
    Just start -> case nextWordPos lb start AfterEnd WordEmacs 1 of
      Nothing -> pure False
      Just end
        | start == end -> pure False
        | otherwise -> do
            word <- drain start end DirForward
            let result = case a of
                  Capitalize -> case graphemes word of
                    (ch : _) -> T.toUpper ch <> T.toLower (dropB (lenB ch) word)
                    [] -> word
                  Lowercase -> T.toLower word
                  Uppercase -> T.toUpper word
            _ <- insertStr start result
            putPos (start + lenB result)
            pure True

-- | Transpose two words.
transposeWords :: (ChangeListener l) => RepeatCount -> LB l Bool
transposeWords n = do
  let wordDef = WordEmacs
  _ <- moveToNextWord AfterEnd wordDef n
  w2End <- cursor
  _ <- moveToPrevWord wordDef 1
  w2Beg <- cursor
  _ <- moveToPrevWord wordDef n
  w1Beg <- cursor
  _ <- moveToNextWord AfterEnd wordDef 1
  w1End <- cursor
  if w1Beg == w2Beg || w2Beg < w1End
    then pure False
    else do
      w1 <- sliceB w1Beg w1End <$> buffer
      w2 <- drain w2Beg w2End DirForward
      _ <- insertStr w2Beg w1
      _ <- drain w1Beg w1End DirForward
      _ <- insertStr w1Beg w2
      putPos w2End
      pure True

-- | Replace the text in @[start, end)@ with @text@ and put the cursor after
--   it.
replace :: (ChangeListener l) => Int -> Int -> Text -> LB l ()
replace start end text = do
  b <- buffer
  notify (onReplace start (sliceB start end b) text)
  putBuf (takeB start b <> text <> dropB end b)
  putPos (start + lenB text)

-- | Insert @s@ at byte index @idx@ (the cursor does not move). Returns
--   whether the text was appended at the end of the line.
insertStr :: (ChangeListener l) => Int -> Text -> LB l Bool
insertStr idx s = do
  notify (onInsertStr idx s)
  b <- buffer
  if idx == lenB b
    then putBuf (b <> s) >> pure True
    else putBuf (insertAt idx s b) >> pure False

-- | Remove the given byte range, leaving the cursor at its start.
deleteRange :: (DeleteListener l) => Int -> Int -> LB l ()
deleteRange start end = do
  setPos start
  () <$ drain start end DirForward

drain :: (DeleteListener l) => Int -> Int -> Direction -> LB l Text
drain start end dir = do
  b <- buffer
  let removed = sliceB start end b
  notify (onDelete start removed dir)
  putBuf (takeB start b <> dropB end b)
  pure removed

-- | The text between the cursor and where the movement would take it.
--   'Nothing' when the buffer is empty or the movement fails.
copy :: Movement -> LineBuffer -> Maybe Text
copy mvt lb@(LineBuffer buf p _ _)
  | isEmpty lb = Nothing
  | otherwise = case mvt of
      WholeLine ->
        let start = startOfLine lb
            end = endOfLine lb
         in if start == end then Nothing else Just (sliceB start p buf)
      BeginningOfLine ->
        let start = startOfLine lb
         in if p == start then Nothing else Just (sliceB start p buf)
      ViFirstPrint
        | p == 0 -> Nothing
        | otherwise -> (\q -> sliceB q p buf) <$> nextWordPos lb 0 Start WordBig 1
      EndOfLine ->
        let end = endOfLine lb
         in if p == end then Nothing else Just (sliceB p end buf)
      EndOfBuffer
        | isCursorAtEnd lb -> Nothing
        | otherwise -> Just (dropB p buf)
      WholeBuffer -> Just buf
      BeginningOfBuffer
        | p == 0 -> Nothing
        | otherwise -> Just (takeB p buf)
      BackwardWord n wordDef -> (\q -> sliceB q p buf) <$> prevWordPos lb p wordDef n
      ForwardWord n at wordDef -> (\q -> sliceB p q buf) <$> nextWordPos lb p at wordDef n
      ViCharSearch n cs ->
        let searchResult = case cs of
              CSForwardBefore c -> searchCharPos lb (CSForward c) n
              _ -> searchCharPos lb cs n
         in (<$> searchResult) $ \q -> case cs of
              CSBackward _ -> sliceB q p buf
              CSBackwardAfter _ -> sliceB q p buf
              CSForwardBefore _ -> sliceB p q buf
              CSForward c -> sliceB p (q + charLenUtf8 c) buf
      BackwardChar n -> (\q -> sliceB q p buf) <$> prevPos n lb
      ForwardChar n -> (\q -> sliceB p q buf) <$> nextPos n lb
      LineUp n -> (\(s, e) -> sliceB s e buf) <$> nLinesUp lb n
      LineDown n -> (\(s, e) -> sliceB s e buf) <$> nLinesDown lb n

-- | Kill the range specified by the movement.
kill :: (DeleteListener l) => Movement -> LB l Bool
kill mvt = do
  let notifying = case mvt of
        ForwardChar _ -> False
        BackwardChar _ -> False
        _ -> True
  when notifying $ notify startKilling
  killed <- case mvt of
    -- Delete (forward) `n` characters at point.
    ForwardChar n -> isJust <$> delete n
    -- Delete `n` characters backward.
    BackwardChar n -> backspace n
    -- Kill the text from point to the end of the line.
    EndOfLine -> killLine
    WholeLine -> moveHome >> killLine
    -- Kill backward from point to the beginning of the line.
    BeginningOfLine -> discardLine
    -- kill `n` words backward (until start of word)
    BackwardWord n wordDef -> deletePrevWord wordDef n
    -- kill `n` words forward (until start/end of word)
    ForwardWord n at wordDef -> deleteWord at wordDef n
    ViCharSearch n cs -> deleteTo cs n
    LineUp n -> killRange nLinesUp n
    LineDown n -> killRange nLinesDown n
    ViFirstPrint -> pure False -- TODO (as in rustyline)
    -- Kill the text from point to the end of the buffer.
    EndOfBuffer -> killBuffer
    -- Kill backward from point to the beginning of the buffer.
    BeginningOfBuffer -> discardBuffer
    WholeBuffer -> moveBufferStart >> killBuffer
  when notifying $ notify stopKilling
  pure killed
  where
    killRange f n = do
      lb <- self
      case f lb n of
        Just (start, end) -> deleteRange start end >> pure True
        Nothing -> pure False

-- | Indent (or dedent) the lines covered by the movement.
indent :: (ChangeListener l) => Movement -> Int -> Bool -> LB l Bool
indent mvt amount dedent = do
  lb@(LineBuffer buf p _ _) <- self
  let pair = case mvt of
        -- all inline operators are the same: indent the current line
        WholeLine -> Just (p, p)
        BeginningOfLine -> Just (p, p)
        ViFirstPrint -> Just (p, p)
        EndOfLine -> Just (p, p)
        BackwardChar _ -> Just (p, p)
        ForwardChar _ -> Just (p, p)
        ViCharSearch _ _ -> Just (p, p)
        EndOfBuffer -> Just (p, lenB buf)
        WholeBuffer -> Just (0, lenB buf)
        BeginningOfBuffer -> Just (0, p)
        BackwardWord n wordDef -> (\q -> (q, p)) <$> prevWordPos lb p wordDef n
        ForwardWord n at wordDef -> (\q -> (p, q)) <$> nextWordPos lb p at wordDef n
        LineUp n -> nLinesUp lb n
        LineDown n -> nLinesDown lb n
      (start0, end0) = fromMaybe (p, p) pair
      start = maybe 0 (+ 1) (rfindB '\n' (takeB start0 buf))
      -- NB: rustyline uses `rfind` here, not `find`
      end = maybe (lenB buf) (+ end0) (rfindB '\n' (dropB end0 buf))
      lines' = T.splitOn "\n" (sliceB start end buf)
  if dedent
    then dedentLines start lines'
    else indentLines start lines'
  pure True
  where
    dedentLines index0 ls = do
      let go _ [] = pure ()
          go index (line : rest) = do
            let maxDel = lenB line - lenB (trimStartRust line)
                deleting = min maxDel amount
            _ <- drain index (index + deleting) DirForward
            p <- cursor
            when (p >= index) $
              if p - index < deleting
                then putPos index -- don't wrap into the previous line
                else putPos (p - deleting)
            go (index + lenB line + 1 - deleting) rest
      go index0 ls
    indentLines index0 ls = do
      let go _ [] = pure ()
          go index (line : rest) = do
            forM_ [0, lenB indentStr .. amount - 1] $ \off ->
              insertStr index (takeB (min (amount - off) (lenB indentStr)) indentStr)
            p <- cursor
            when (p >= index) $ putPos (p + amount)
            go (index + amount + lenB line + 1) rest
      go index0 ls

isStartOfWord :: Word -> Text -> Text -> Bool
isStartOfWord wordDef previous grapheme =
  (not (isWordChar wordDef previous) && isWordChar wordDef grapheme)
    || (wordDef == WordVi && not (isOtherChar previous) && isOtherChar grapheme)

isEndOfWord :: Word -> Text -> Text -> Bool
isEndOfWord wordDef grapheme next =
  (not (isWordChar wordDef next) && isWordChar wordDef grapheme)
    || (wordDef == WordVi && not (isOtherChar next) && isOtherChar grapheme)

isWordChar :: Word -> Text -> Bool
isWordChar wordDef grapheme = case wordDef of
  WordEmacs -> T.all isAlphanumeric grapheme
  WordVi -> isViWordChar grapheme
  WordBig -> not (T.any isWhitespace grapheme)

isViWordChar :: Text -> Bool
isViWordChar grapheme = T.all isAlphanumeric grapheme || grapheme == "_"

isOtherChar :: Text -> Bool
isOtherChar grapheme = not (T.any isWhitespace grapheme || isViWordChar grapheme)
