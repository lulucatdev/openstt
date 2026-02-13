# BDD Specifications

```gherkin
Feature: Auto-Update for OpenSTT
  As an OpenSTT user on macOS Apple Silicon
  I want the app to check for updates and let me install them in one click
  So that I always have the latest version without manual DMG downloads

  Background:
    Given the app is running on macOS Apple Silicon
    And the current app version is "1.0.7"
    And the updater endpoint is configured to GitHub Releases

  # ── Startup auto-check ──

  Scenario: No update available on startup
    Given the latest version on GitHub Releases is "1.0.7"
    When the app launches
    Then it silently checks for updates in the background
    And no update indicator is shown anywhere in the UI
    And the app functions normally

  Scenario: Update available on startup
    Given the latest version on GitHub Releases is "1.0.8"
    And the release notes are "Bug fixes and improvements"
    When the app launches
    Then it silently checks for updates in the background
    And when the user opens Settings > About
    Then an update banner shows "v1.0.8" with install button

  Scenario: Network error on startup check
    Given the device has no internet connection
    When the app launches
    Then the update check fails silently
    And no error is shown to the user
    And the app functions normally

  # ── Manual check ──

  Scenario: Manual check finds no update
    Given the latest version on GitHub Releases is "1.0.7"
    When the user clicks "Check for Updates"
    Then a loading spinner is shown on the button
    And after the check completes, a message shows "You're up to date"
    And the message disappears after a few seconds

  Scenario: Manual check finds update
    Given the latest version on GitHub Releases is "1.0.8"
    When the user clicks "Check for Updates"
    Then a loading spinner is shown on the button
    And after the check completes, an update banner appears
    And the banner shows version "1.0.8" and release notes
    And an "Install Update" button is displayed

  Scenario: Manual check with network error
    Given the device has no internet connection
    When the user clicks "Check for Updates"
    Then after a timeout, an error message is shown
    And the user can retry by clicking the button again

  # ── Install update ──

  Scenario: Successful update installation
    Given an update to version "1.0.8" is available
    When the user clicks "Install Update"
    Then a progress bar appears showing download percentage
    And the "Install Update" button is disabled during download
    When the download completes
    Then the app restarts automatically
    And after restart the version is "1.0.8"

  Scenario: Download fails mid-way
    Given an update to version "1.0.8" is available
    And the user clicks "Install Update"
    When the download fails at 45%
    Then the progress bar stops
    And an error message is shown
    And a "Retry" button appears
    When the user clicks "Retry"
    Then the download restarts

  # ── Version comparison ──

  Scenario Outline: Version comparison
    Given the current version is "<current>"
    And the latest version is "<latest>"
    When checking for updates
    Then update available is "<available>"

    Examples:
      | current | latest | available |
      | 1.0.7   | 1.0.8  | yes       |
      | 1.0.7   | 1.0.7  | no        |
      | 1.0.8   | 1.0.7  | no        |
      | 1.0.7   | 1.1.0  | yes       |
      | 1.0.7   | 2.0.0  | yes       |

  # ── i18n ──

  Scenario: Chinese language support
    Given the app language is set to Chinese
    When the user opens Settings > About
    Then the "Check for Updates" button shows "检查更新"
    And if an update is available, the banner shows "有新版本"
    And the install button shows "安装更新"
    And "up to date" message shows "已是最新版本"
```
