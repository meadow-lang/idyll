-- | Screen layout bookkeeping (@layout.rs@).
module System.Console.Rustyline.Layout
  ( GraphemeClusterMode (..),
    gcmWidth,
    Unit,
    cwidh,
    Position (..),
    origin,
    Layout (..),
    newLayout,
    layoutWidth,
  )
where

import Data.Maybe (fromMaybe)
import Data.Text (Text)
import Data.Text qualified as T
import System.Console.Rustyline.Config (GraphemeClusterMode (..))
import System.Console.Rustyline.Internal.Unicode.Width (charWidth, strWidth)

-- | Height, width. (Rust uses @u16@; plain 'Int' here.)
type Unit = Int

-- | Grapheme width / number of columns.
gcmWidth :: GraphemeClusterMode -> Text -> Unit
gcmWidth mode s = case mode of
  Unicode -> strWidth s
  WcWidth -> sum (map cwidh (T.unpack s))
  NoZwj -> sum (map strWidth (T.splitOn "\x200D" s))

-- | Character width / number of columns (control characters are 0 wide).
cwidh :: Char -> Unit
cwidh c = fromMaybe 0 (charWidth c)

-- | A screen position. The leftmost column and the highest row are 0.
--   Ordered by row, then column.
data Position = Position
  { posCol :: Unit,
    posRow :: Unit
  }
  deriving (Eq, Show)

instance Ord Position where
  compare a b = compare (posRow a, posCol a) (posRow b, posCol b)

data Layout = Layout
  { layoutGcm :: GraphemeClusterMode,
    -- | Prompt Unicode/visible width and height
    layoutPromptSize :: Position,
    layoutDefaultPrompt :: Bool,
    -- | Cursor position (relative to the start of the prompt)
    layoutCursor :: Position,
    -- | Number of rows used so far (from start of prompt to end of input)
    layoutEnd :: Position,
    -- | Has some hint or message at the end of input
    layoutHasInfo :: Bool
  }
  deriving (Eq, Show)

origin :: Position
origin = Position 0 0

newLayout :: GraphemeClusterMode -> Layout
newLayout gcm =
  Layout
    { layoutGcm = gcm,
      layoutPromptSize = Position 0 0,
      layoutDefaultPrompt = False,
      layoutCursor = Position 0 0,
      layoutEnd = Position 0 0,
      layoutHasInfo = False
    }

layoutWidth :: Layout -> Text -> Unit
layoutWidth l = gcmWidth (layoutGcm l)
