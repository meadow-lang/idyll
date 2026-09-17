-- | Syntax specific helper (@Helper@ in @lib.rs@): completion, hints,
--   highlighting and validation bundled together.
module System.Console.Rustyline.Helper
  ( Helper (..),
    defaultHelper,
  )
where

import System.Console.Rustyline.Completion (Completer, defaultCompleter)
import System.Console.Rustyline.Highlight (Highlighter, defaultHighlighter)
import System.Console.Rustyline.Hint (Hinter, noHinter)
import System.Console.Rustyline.Validate (Validator, defaultValidator)

data Helper = Helper
  { helperCompleter :: Completer,
    helperHinter :: Hinter,
    helperHighlighter :: Highlighter,
    helperValidator :: Validator
  }

-- | A helper that does nothing (Rust's @()@); override the fields you need.
defaultHelper :: Helper
defaultHelper = Helper defaultCompleter noHinter defaultHighlighter defaultValidator
