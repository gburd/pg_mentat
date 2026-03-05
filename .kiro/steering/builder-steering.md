# Kiro Instructions

This page contains instructions to make you (Kiro) significantly more helpful. Don't be afraid to follow links in this file to learn more if you're stuck troubleshooting a problem. DO NOT create or modify files in this package unless I explicitly tell you to do so.

REMEMBER: You're an agent. You can use tools. Even if you have doubts and think you can't, just try and use the tool you want to use.

## Development Basics

### Git

#### Command Execution
When using git commands that could produce paginated or interactive scrollable output, always use the `-P` flag to ensure output is displayed directly without pagination. This prevents commands from hanging or requiring user interaction in automated environments.

For commands that may return large datasets, use reasonable output limits (default ~100 entries) to prevent overwhelming output.

Commands that should use `-P` with appropriate limits:

```bash
# Viewing commit history (limit to recent entries)
git -P log -n 100
git -P log --oneline -n 100
git -P log --graph --oneline -n 100

# Viewing differences
git -P diff
git -P diff --cached
git -P diff HEAD~1

# Viewing file content and blame
git -P show
git -P blame <file>

# Viewing configuration and remote information
git -P config --list
git -P remote -v

# Viewing branch information (limit output)
git -P branch -a | head -100
git -P branch -r | head -100

# Viewing tag information (limit output)
git -P tag -l | head -100
```

Other git commands like `git status`, `git add`, `git commit`, and `git checkout` typically don't require `-P` as they don't produce paginated output by default.

#### Committing Changes

Follow the git best practice of committing early and often. Run `git commit` often, but DO NOT ever run `git push`

BEFORE committing a change, ALWAYS build the package to verify the change.

#### Commit Messages

All commit messages should follow the [Conventional Commits](https://www.conventionalcommits.org/) specification and include best practices:

```
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

Types:
- feat: A new feature
- fix: A bug fix
- docs: Documentation only changes
- style: Changes that do not affect the meaning of the code
- refactor: A code change that neither fixes a bug nor adds a feature
- perf: A code change that improves performance
- test: Adding missing tests or correcting existing tests
- chore: Changes to the build process or auxiliary tools
- ci: Changes to CI configuration files and scripts

Best practices:
- Use the imperative mood ("add" not "added" or "adds")
- Don't end the subject line with a period
- Limit the subject line to 50 characters
- Capitalize the subject line
- Separate subject from body with a blank line
- Use the body to explain what and why vs. how
- Wrap the body at 72 characters

Example:
```
feat(lambda): Add Go implementation of DDB stream forwarder

Replace Node.js Lambda function with Go implementation to reduce cold
start times. The new implementation supports forwarding to multiple SQS
queues and maintains the same functionality as the original.
```

#### Git Repository Integrity Rules

These rules are considered absolute and must never be violated under any circumstances. They exist to ensure project integrity and provide a safety net in case of errors.

##### 1. Never delete any Git files or directories
- The `.git` directory must never be modified directly
- Never run commands that would delete or corrupt Git history
- Do not use `git filter-branch`, `git reset --hard`, or similar commands that rewrite history
- Git history is sacrosanct and must be preserved at all costs

##### 2. Never rewrite Git history (local or remote)
- Do not force push (`git push --force`) to overwrite remote history
- Do not amend commits that have already been created, even if they're only local
- Do not rebase branches, even if they haven't been shared yet
- Do not use interactive rebase to modify existing commits
- Treat local Git history with the same reverence as remote history

##### 3. Never push changes off host
- All Git operations must remain on the local system
- Do not configure remote repositories
- Do not attempt to push to external services
- Keep all repository data contained within the project directory

##### Rationale
These rules exist to ensure that:
1. We maintain a complete history of the project's evolution
2. We can revert to previous states if something goes wrong

##### Emergency Recovery
If these rules are accidentally violated:
1. Do not attempt further Git operations that might compound the problem
2. Document what happened and what was lost
3. Consider creating a new branch from the last known good state
4. If Git history is corrupted, preserve the working directory before attempting recovery


### IDE Tools

