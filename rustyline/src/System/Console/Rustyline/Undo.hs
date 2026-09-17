-- | Undo API (@undo.rs@).
module System.Console.Rustyline.Undo
  ( Changeset,
    newChangeset,
    begin,
    end,
    insertChange,
    insertStrChange,
    deleteChange,
    replaceChange,
    undo,
    redo,
    truncateChanges,
    lastInsert,
    -- * Test introspection
    undoCount,
    redoCount,
  )
where

import Data.Text (Text)
import Data.Text qualified as T
import System.Console.Rustyline.Internal.Text (lenB)
import System.Console.Rustyline.Internal.Unicode.CharProps (isAlphanumeric)
import System.Console.Rustyline.Internal.Unicode.Grapheme (graphemes)
import System.Console.Rustyline.LineBuffer
import System.Console.Rustyline.Keymap.Types (RepeatCount)

data Change
  = Begin
  | End
  | -- | QuotedInsert, SelfInsert, Yank
    ChInsert Int Text
  | -- | BackwardDeleteChar, BackwardKillWord, DeleteChar, KillLine,
    --   KillWholeLine, KillWord, UnixLikeDiscard, ViDeleteTo
    ChDelete Int Text
  | -- | CapitalizeWord, Complete, DowncaseWord, Replace, TransposeChars,
    --   TransposeWords, UpcaseWord, YankPop
    ChReplace Int Text Text
  deriving (Eq, Show)

changeUndo :: Change -> LB NoListener ()
changeUndo = \case
  ChInsert idx text -> deleteRange idx (idx + lenB text)
  ChDelete idx text -> do
    _ <- insertStr idx text
    setPos (idx + lenB text)
  ChReplace idx old new -> replace idx (idx + lenB new) old
  _ -> error "changeUndo: Begin/End"

changeRedo :: Change -> LB NoListener ()
changeRedo = \case
  ChInsert idx text -> () <$ insertStr idx text
  ChDelete idx text -> deleteRange idx (idx + lenB text)
  ChReplace idx old new -> replace idx (idx + lenB old) new
  _ -> error "changeRedo: Begin/End"

insertSeq :: Int -> Change -> Bool
insertSeq indx = \case
  ChInsert idx text -> idx + lenB text == indx
  _ -> False

deleteSeq :: Int -> Int -> Change -> Bool
deleteSeq indx l = \case
  -- delete or backspace
  ChDelete idx _ -> idx == indx || idx == indx + l
  _ -> False

replaceSeq :: Int -> Change -> Bool
replaceSeq indx = \case
  ChReplace idx _ new -> idx + lenB new == indx
  _ -> False

-- | Undo manager. The change stacks keep the most recent change first.
data Changeset = Changeset
  { undoGroupLevel :: !Int,
    undos :: [Change],
    redos :: [Change]
  }
  deriving (Eq, Show)

newChangeset :: Changeset
newChangeset = Changeset 0 [] []

-- | Open an undo group; returns a mark for 'truncateChanges'.
begin :: Changeset -> (Int, Changeset)
begin cs =
  let mark = length (undos cs)
   in (mark, cs {redos = [], undos = Begin : undos cs, undoGroupLevel = undoGroupLevel cs + 1})

-- | Close all open groups. True when something changed since 'begin'.
end :: Changeset -> (Bool, Changeset)
end cs0 = go False cs0 {redos = []}
  where
    go touched cs
      | undoGroupLevel cs <= 0 = (touched, cs)
      | otherwise =
          let cs' = cs {undoGroupLevel = undoGroupLevel cs - 1}
           in case undos cs' of
                -- empty Begin..End
                (Begin : rest) -> go touched cs' {undos = rest}
                us -> go True cs' {undos = End : us}

insertChange :: Int -> Char -> Changeset -> Changeset
insertChange idx c cs0 =
  let cs = cs0 {redos = []}
   in case undos cs of
        (ChInsert i text : rest)
          | isAlphanumeric c && insertSeq idx (ChInsert i text) ->
              -- merge consecutive char insertions when char is alphanumeric
              cs {undos = ChInsert i (T.snoc text c) : rest}
        us -> cs {undos = ChInsert idx (T.singleton c) : us}

insertStrChange :: Int -> Text -> Changeset -> Changeset
insertStrChange idx string cs0
  | T.null string = cs
  | otherwise = cs {undos = ChInsert idx string : undos cs}
  where
    cs = cs0 {redos = []}

deleteChange :: Int -> Text -> Changeset -> Changeset
deleteChange indx string cs0
  | T.null string = cs
  | otherwise = case undos cs of
      (lc@(ChDelete idx text) : rest)
        | singleChar string && deleteSeq indx (lenB string) lc ->
            -- merge consecutive char deletions when char is alphanumeric
            let merged =
                  if idx == indx
                    then ChDelete idx (text <> string)
                    else ChDelete indx (string <> text)
             in cs {undos = merged : rest}
      us -> cs {undos = ChDelete indx string : us}
  where
    cs = cs0 {redos = []}

singleChar :: Text -> Bool
singleChar s = case graphemes s of
  [g] -> T.all isAlphanumeric g
  _ -> False

replaceChange :: Int -> Text -> Text -> Changeset -> Changeset
replaceChange indx old new cs0 = case undos cs of
  (lc@(ChReplace idx o n) : rest)
    | replaceSeq indx lc ->
        -- merge consecutive char replacements
        cs {undos = ChReplace idx (o <> old) (n <> new) : rest}
  us -> cs {undos = ChReplace indx old new : us}
  where
    cs = cs0 {redos = []}

-- | Undo @n@ groups of changes. Returns whether anything was undone.
undo :: RepeatCount -> Changeset -> LineBuffer -> (Bool, Changeset, LineBuffer)
undo n = go (0 :: Int) (0 :: Int) False
  where
    go count waiting undone cs lb = case undos cs of
      [] -> (undone, cs, lb)
      (change : rest) ->
        let (waiting', undone', lb') = case change of
              Begin -> (waiting - 1, undone, lb)
              End -> (waiting + 1, undone, lb)
              _ -> (waiting, True, snd (evalLB NoListener lb (changeUndo change)))
            cs' = cs {undos = rest, redos = change : redos cs}
         in if waiting' <= 0
              then
                let count' = count + 1
                 in if count' >= n
                      then (undone', cs', lb')
                      else go count' waiting' undone' cs' lb'
              else go count waiting' undone' cs' lb'

-- | Redo one group of changes (used by tests, as in rustyline).
redo :: Changeset -> LineBuffer -> (Bool, Changeset, LineBuffer)
redo = go (0 :: Int) False
  where
    go waiting redone cs lb = case redos cs of
      [] -> (redone, cs, lb)
      (change : rest) ->
        let (waiting', redone', lb') = case change of
              Begin -> (waiting + 1, redone, lb)
              End -> (waiting - 1, redone, lb)
              _ -> (waiting, True, snd (evalLB NoListener lb (changeRedo change)))
            cs' = cs {redos = rest, undos = change : undos cs}
         in if waiting' <= 0 then (redone', cs', lb') else go waiting' redone' cs' lb'

-- | Keep only the oldest @n@ undo entries.
truncateChanges :: Int -> Changeset -> Changeset
truncateChanges n cs = cs {undos = drop (length (undos cs) - n) (undos cs)}

-- | The text of the most recent insertion, if the last change was one.
lastInsert :: Changeset -> Maybe Text
lastInsert = go . undos
  where
    go (ChInsert _ text : _) = Just text
    go (ChReplace _ _ new : _) = Just new
    go (End : rest) = go rest
    go _ = Nothing

undoCount :: Changeset -> Int
undoCount = length . undos

redoCount :: Changeset -> Int
redoCount = length . redos

instance DeleteListener Changeset where
  onDelete idx string _ = deleteChange idx string

instance ChangeListener Changeset where
  onInsertChar = insertChange
  onInsertStr = insertStrChange
  onReplace = replaceChange
