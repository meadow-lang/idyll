-- | Input validation API, for multi-line editing (@validate.rs@).
module System.Console.Rustyline.Validate
  ( ValidationResult (..),
    isValid,
    hasMessage,
    ValidationContext (..),
    Validator (..),
    defaultValidator,
    matchingBracketValidator,
    validateBrackets,
  )
where

import Data.Text (Text)
import Data.Text qualified as T

-- | Input validation result.
data ValidationResult
  = -- | Incomplete input
    Incomplete
  | -- | Validation fails with an optional error message. The user must fix
    --   the input.
    Invalid (Maybe Text)
  | -- | Validation succeeds with an optional message
    Valid (Maybe Text)
  deriving (Eq, Show)

isValid :: ValidationResult -> Bool
isValid = \case
  Valid _ -> True
  _ -> False

hasMessage :: ValidationResult -> Bool
hasMessage = \case
  Valid (Just _) -> True
  Invalid (Just _) -> True
  _ -> False

-- | Gives access to the user input.
newtype ValidationContext = ValidationContext
  { -- | The user input.
    validationInput :: Text
  }

-- | Decides whether hitting Enter ends the editing session (returning the
--   line to the caller of @readline@).
data Validator = Validator
  { -- | Takes the currently edited input and says whether it is valid,
    --   with an optional message to display.
    validate :: ValidationContext -> IO ValidationResult,
    -- | Validate while typing or only on Enter (not yet implemented, as in
    --   rustyline).
    validateWhileTyping :: Bool
  }

-- | Everything is valid.
defaultValidator :: Validator
defaultValidator = Validator (\_ -> pure (Valid Nothing)) False

-- | Simple matching bracket validator.
matchingBracketValidator :: Validator
matchingBracketValidator = defaultValidator {validate = pure . validateBrackets . validationInput}

validateBrackets :: Text -> ValidationResult
validateBrackets = go [] . T.unpack
  where
    go stack [] = if null stack then Valid Nothing else Incomplete
    go stack (c : cs)
      | c `elem` ("([{" :: String) = go (c : stack) cs
      | c `elem` (")]}" :: String) = case stack of
          (o : rest)
            | matches o c -> go rest cs
            | otherwise ->
                Invalid (Just ("Mismatched brackets: " <> T.pack (show o) <> " is not properly closed"))
          [] -> Invalid (Just ("Mismatched brackets: " <> T.pack (show c) <> " is unpaired"))
      | otherwise = go stack cs
    matches '(' ')' = True
    matches '[' ']' = True
    matches '{' '}' = True
    matches _ _ = False
