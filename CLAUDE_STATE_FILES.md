# Claude Code State Files for Migration

## Critical Files to Transfer

When moving this project to a Linux machine, copy these Claude Code state files to preserve full context:

### 1. Conversation Transcript (Full History)
**Location:** `/Users/gregburd/.claude/projects/-Users-gregburd-src-mentat/f24219f8-c170-4c91-855e-e05233bfc06f.jsonl`

**Size:** ~31,835 tokens (large file)

**Contains:**
- Complete implementation conversation
- All decisions and rationale
- Agent interactions and task assignments
- Problem-solving discussions
- Code iterations and fixes

**How to transfer:**
```bash
# Copy transcript to repository
cp /Users/gregburd/.claude/projects/-Users-gregburd-src-mentat/f24219f8-c170-4c91-855e-e05233bfc06f.jsonl \
   /Users/gregburd/src/mentat/.claude-transcript.jsonl

# Include in git
git add .claude-transcript.jsonl
git commit -m "Add Claude conversation transcript for context"
```

**On Linux:**
```bash
# Restore to Claude directory (if using Claude Code)
mkdir -p ~/.claude/projects/-Users-gregburd-src-mentat/
cp .claude-transcript.jsonl \
   ~/.claude/projects/-Users-gregburd-src-mentat/f24219f8-c170-4c91-855e-e05233bfc06f.jsonl
```

### 2. Migration Plan
**Location:** `/Users/gregburd/.claude/plans/wiggly-singing-wigderson.md`

**Already copied to repository:** `.claude-state-plan.md`

**Contains:**
- Original 6-month migration plan
- All 10 phases detailed
- Task breakdown (20 tasks)
- Implementation timeline
- Design decisions

### 3. Team Configuration
**Location:** `/Users/gregburd/.claude/teams/mentat-migration/config.json`

**Contains:**
- Team member definitions
- Agent roles and prompts
- Task assignments
- Agent IDs and metadata

**Copy command:**
```bash
cp /Users/gregburd/.claude/teams/mentat-migration/config.json \
   /Users/gregburd/src/mentat/.claude-team-config.json

git add .claude-team-config.json
git commit -m "Add team configuration"
```

### 4. Task List State
**Location:** `/Users/gregburd/.claude/tasks/mentat-migration/`

**Contains:**
- Task definitions
- Task status (pending, in_progress, completed)
- Task dependencies
- Task metadata

**Copy command:**
```bash
# Copy entire task directory
cp -r /Users/gregburd/.claude/tasks/mentat-migration/ \
      /Users/gregburd/src/mentat/.claude-tasks/

git add .claude-tasks/
git commit -m "Add task list state"
```

---

## Quick Transfer Script

```bash
#!/bin/bash
# Run from mentat repository root

# Copy all Claude state files to repository
cp /Users/gregburd/.claude/projects/-Users-gregburd-src-mentat/f24219f8-c170-4c91-855e-e05233bfc06f.jsonl \
   .claude-transcript.jsonl

cp /Users/gregburd/.claude/teams/mentat-migration/config.json \
   .claude-team-config.json

cp -r /Users/gregburd/.claude/tasks/mentat-migration/ \
      .claude-tasks/

# Stage and commit
git add .claude-transcript.jsonl .claude-team-config.json .claude-tasks/
git commit -m "Add Claude Code state files for migration"

echo "Claude state files added to repository"
echo "Push to remote and clone on Linux machine"
```

---

## Using Claude Code on Linux

If you want to continue with Claude Code on the Linux machine:

### 1. Install Claude Code
```bash
# Follow official Claude Code installation for Linux
# https://docs.anthropic.com/claude-code
```

### 2. Restore State Files
```bash
cd ~/mentat  # Your cloned repository

# Create Claude directories
mkdir -p ~/.claude/projects/-Users-gregburd-src-mentat/
mkdir -p ~/.claude/teams/mentat-migration/
mkdir -p ~/.claude/tasks/

# Restore files
cp .claude-transcript.jsonl \
   ~/.claude/projects/-Users-gregburd-src-mentat/f24219f8-c170-4c91-855e-e05233bfc06f.jsonl

cp .claude-team-config.json \
   ~/.claude/teams/mentat-migration/config.json

cp -r .claude-tasks/* \
      ~/.claude/tasks/mentat-migration/
```

### 3. Resume Work
```bash
# Open project in Claude Code
claude-code ~/mentat

# Claude will load previous context automatically
# Reference MIGRATION_GUIDE.md for next steps
```

---

## Alternative: Work Without Claude Code

If NOT using Claude Code on Linux:

**All necessary context is in the repository:**
- `MIGRATION_GUIDE.md` - Comprehensive continuation guide
- `HANDOFF_SUMMARY.md` - Quick start summary
- `docs/architecture/` - Complete architecture documentation
- Code is self-documenting with comments

**You don't need the state files** - they're just helpful if continuing with Claude Code.

---

## State File Manifest

| File | Purpose | Size | Required |
|------|---------|------|----------|
| `f24219f8-*.jsonl` | Conversation transcript | 31,835 tokens | Optional (for Claude Code) |
| `.claude-state-plan.md` | Original migration plan | Already in repo | ✅ Included |
| `config.json` | Team configuration | Small | Optional (for Claude Code) |
| `tasks/` | Task list state | Small | Optional (for Claude Code) |

---

## Summary

**Minimum transfer (no Claude Code on Linux):**
- Just the git repository (all code and docs included)

**Full transfer (using Claude Code on Linux):**
- Git repository
- `.claude-transcript.jsonl` (conversation history)
- `.claude-team-config.json` (team setup)
- `.claude-tasks/` (task state)

**Recommendation:**
- Transfer transcript even if not using Claude Code
- Useful for understanding decisions and context
- Only 31KB file with full history

---

**Next step:** Run the quick transfer script above, commit, and push to remote.
