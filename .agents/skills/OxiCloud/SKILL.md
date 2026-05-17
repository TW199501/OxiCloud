```markdown
# OxiCloud Development Patterns

> Auto-generated skill from repository analysis

## Overview
This skill teaches the core development patterns and conventions used in the OxiCloud JavaScript codebase. It covers file naming, import/export styles, commit message conventions, and testing patterns. By following these guidelines, contributors can write consistent, maintainable code and collaborate effectively.

## Coding Conventions

### File Naming
- Use **camelCase** for file names.
  - Example: `userProfile.js`, `dataFetcher.js`

### Import Style
- Use **relative imports** for modules within the project.
  - Example:
    ```javascript
    import { fetchData } from './apiUtils';
    ```

### Export Style
- Use **named exports**.
  - Example:
    ```javascript
    // In apiUtils.js
    export function fetchData() { ... }
    export const API_URL = '...';
    ```

### Commit Messages
- Follow the **conventional commit** style.
- Use the `feat` prefix for new features.
- Keep commit messages concise (average 50 characters).
  - Example:
    ```
    feat: add user authentication flow
    ```

## Workflows

### Feature Development
**Trigger:** When implementing a new feature  
**Command:** `/feature`

1. Create a new branch for the feature.
2. Write code using camelCase file naming and relative imports.
3. Use named exports for all modules.
4. Write or update tests in corresponding `*.test.*` files.
5. Commit changes using the `feat` prefix and a concise message.
6. Open a pull request for review.

### Testing
**Trigger:** Before merging or releasing code  
**Command:** `/test`

1. Identify all `*.test.*` files.
2. Run tests using the project's preferred method (framework unknown; see project docs).
3. Ensure all tests pass before proceeding.

## Testing Patterns

- Test files follow the `*.test.*` naming convention.
  - Example: `userProfile.test.js`
- The testing framework is not specified; refer to project documentation or existing test files for guidance.
- Place tests alongside the modules they cover or in a dedicated test directory.

## Commands
| Command   | Purpose                                      |
|-----------|----------------------------------------------|
| /feature  | Start a new feature development workflow     |
| /test     | Run all tests before merging or releasing    |
```
