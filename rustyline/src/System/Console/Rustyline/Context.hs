{-# LANGUAGE ExistentialQuantification #-}

-- | Completion/suggestion context (@Context@ in @lib.rs@).
module System.Console.Rustyline.Context
  ( AnyHistory (..),
    Context (..),
    newContext,
  )
where

import System.Console.Rustyline.History

-- | Any history backend (Rust's @&dyn History@).
data AnyHistory = forall h. (History h) => AnyHistory h

instance History AnyHistory where
  historyGet (AnyHistory h) = historyGet h
  historyAdd (AnyHistory h) = historyAdd h
  historyLen (AnyHistory h) = historyLen h
  historyIsEmpty (AnyHistory h) = historyIsEmpty h
  historySetMaxLen (AnyHistory h) = historySetMaxLen h
  historySetIgnoreDups (AnyHistory h) = historySetIgnoreDups h
  historySetIgnoreSpace (AnyHistory h) = historySetIgnoreSpace h
  historySave (AnyHistory h) = historySave h
  historyAppend (AnyHistory h) = historyAppend h
  historyLoad (AnyHistory h) = historyLoad h
  historyClear (AnyHistory h) = historyClear h
  historySearch (AnyHistory h) = historySearch h
  historyStartsWith (AnyHistory h) = historyStartsWith h
  historyEntries (AnyHistory h) = historyEntries h

-- | Gives helpers access to the history.
data Context = Context
  { -- | The history object.
    ctxHistory :: AnyHistory,
    -- | The history index we are currently editing.
    ctxHistoryIndex :: Int
  }

-- | A context positioned after the last history entry.
newContext :: (History h) => h -> IO Context
newContext h = Context (AnyHistory h) <$> historyLen h
