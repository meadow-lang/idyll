-- | Kill ring management (@kill_ring.rs@).
module System.Console.Rustyline.KillRing
  ( KillRing (..),
    KillAction (..),
    KillMode (..),
    newKillRing,
    resetKillRing,
    killText,
    yankText,
    yankPopText,
  )
where

import Data.Sequence (Seq)
import Data.Sequence qualified as Seq
import Data.Text (Text)
import System.Console.Rustyline.Internal.Text (lenB)
import System.Console.Rustyline.LineBuffer (DeleteListener (..), Direction (..))

data KillAction
  = KAKill
  | KAYank Int
  | KAOther
  deriving (Eq, Show)

data KillMode
  = Append
  | Prepend
  deriving (Eq, Show)

data KillRing = KillRing
  { krSize :: !Int,
    krSlots :: Seq Text,
    -- | where we are in the kill ring
    krIndex :: !Int,
    -- | whether or not the last command was a kill or a yank
    krLastAction :: !KillAction,
    krKilling :: !Bool
  }
  deriving (Show)

-- | Create a new kill ring of the given size.
newKillRing :: Int -> KillRing
newKillRing size = KillRing size Seq.empty 0 KAOther False

-- | Reset the last-action state.
resetKillRing :: KillRing -> KillRing
resetKillRing kr = kr {krLastAction = KAOther}

-- | Add @text@ to the kill ring.
killText :: Text -> KillMode -> KillRing -> KillRing
killText text dir kr
  | krLastAction kr == KAKill =
      if krSize kr == 0
        then kr -- disabled
        else
          let f s = case dir of
                Append -> s <> text
                Prepend -> text <> s
           in kr {krSlots = Seq.adjust' f (krIndex kr) (krSlots kr)}
  | otherwise =
      let kr' = kr {krLastAction = KAKill}
       in if krSize kr' == 0
            then kr' -- disabled
            else
              let index
                    | krIndex kr' == krSize kr' - 1 = 0 -- full
                    | not (Seq.null (krSlots kr')) = krIndex kr' + 1
                    | otherwise = krIndex kr'
                  slots
                    | index == Seq.length (krSlots kr') = krSlots kr' Seq.|> text
                    | otherwise = Seq.update index text (krSlots kr')
               in kr' {krIndex = index, krSlots = slots}

-- | Yank previously killed text. 'Nothing' when the kill ring is empty.
yankText :: KillRing -> (Maybe Text, KillRing)
yankText kr = case Seq.lookup (krIndex kr) (krSlots kr) of
  Nothing -> (Nothing, kr)
  Just t -> (Just t, kr {krLastAction = KAYank (lenB t)})

-- | Yank the text in the previous slot, replacing the last yank of the given
--   size. 'Nothing' when the previous command was not a yank.
yankPopText :: KillRing -> (Maybe (Int, Text), KillRing)
yankPopText kr = case krLastAction kr of
  KAYank yankSize
    | Seq.null (krSlots kr) -> (Nothing, kr)
    | otherwise ->
        let index = if krIndex kr == 0 then Seq.length (krSlots kr) - 1 else krIndex kr - 1
            t = Seq.index (krSlots kr) index
         in (Just (yankSize, t), kr {krIndex = index, krLastAction = KAYank (lenB t)})
  _ -> (Nothing, kr)

instance DeleteListener KillRing where
  startKilling kr = kr {krKilling = True}
  onDelete _ string dir kr
    | not (krKilling kr) = kr
    | otherwise = killText string mode kr
    where
      mode = case dir of
        DirForward -> Append
        DirBackward -> Prepend
  stopKilling kr = kr {krKilling = False}
