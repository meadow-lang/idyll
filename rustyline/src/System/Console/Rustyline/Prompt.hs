-- | Prompts (@prompt.rs@).
module System.Console.Rustyline.Prompt
  ( Prompt (..),
    prompt,
  )
where

import Data.String (IsString (..))
import Data.Text (Text)
import Data.Text qualified as T

-- | Two versions of the prompt: the raw one, used when stdout is not a tty
--   or colours are disabled, and a styled one (with ANSI escapes) which must
--   have the same display width.
data Prompt = Prompt
  { promptRaw :: Text,
    promptStyled :: Text
  }
  deriving (Eq, Show)

-- | A prompt whose styled version is the raw text.
prompt :: Text -> Prompt
prompt t = Prompt t t

instance IsString Prompt where
  fromString = prompt . T.pack
