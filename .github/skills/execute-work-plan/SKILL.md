---
name: execute-work-plan
description: 'Execute tasks from a repository-root WORK_PLAN.md and record verified changes, tests, blockers, and next steps in WORK_PROGRESS.md. Use for carrying out a project work plan or resuming tracked implementation work.'
argument-hint: 'Name a plan phase or task to execute, or ask to continue the plan.'
---

# Execute a Work Plan

## Purpose

Carry out the actionable work in the repository-root `WORK_PLAN.md` and maintain an accurate execution log in the repository-root `WORK_PROGRESS.md`. The plan defines intended work; the progress log records what actually happened and what evidence supports it.

## Workflow

1. Read `WORK_PLAN.md` and `WORK_PROGRESS.md` before acting. If the plan is missing, empty, or too ambiguous to execute safely, ask for clarification instead of inventing tasks. If the progress file is missing, create it when the first work item is started.
2. Select the next incomplete task in dependency order. Honor explicit user-selected scope, existing decisions, and completed work recorded in either file. Do not repeat completed tasks unless the user asks for revalidation.
3. Before each task, inspect only the nearby code, tests, documentation, or configuration needed to verify the plan's assumptions. If the planned change conflicts with the current repository state, stop and update the progress log with the discrepancy before asking how to proceed.
4. Execute one task at a time. For code changes, identify the behavior-owning path, make the smallest focused change, and run a focused check immediately. For documentation or configuration work, use the relevant parser, check, or review criteria available in the repository.
5. After each task, update `WORK_PROGRESS.md` with:
   - The plan phase/task completed or attempted
   - A concise summary of changes and affected files
   - Exact validation commands and whether they passed or failed
   - Any remaining risks, blockers, or decisions
   - The next incomplete task
6. Continue sequentially through unblocked tasks within the current phase. At the end of each phase, update `WORK_PROGRESS.md` and pause for user review before starting the next phase. Also pause for user input when a material product decision, destructive operation, external access, or unresolved plan conflict is required. If a check fails, diagnose and repair the same task when feasible, then rerun the focused check; record unresolved failures honestly.
7. Mark a task complete only after its stated acceptance criteria have been verified. A code edit without successful validation is in progress or blocked, not complete. Do not claim a whole phase or plan is complete while any required task or acceptance check remains open.
8. Preserve the plan as the source of intended scope. Do not silently rewrite or remove plan items; if scope must change, explain the reason in `WORK_PROGRESS.md` and ask before changing the plan.
9. At the end of the run, review the plan and progress log for accurate statuses, validation evidence, unresolved blockers, and a clear next action. Run `git diff --check` when files were edited and report the exact overall outcome.

## Progress Log Format

Use dated entries grouped by plan phase or task. Prefer concise, evidence-based notes over copying the full plan. Keep prior entries and user-authored content; append a new entry or update the relevant in-progress entry instead of replacing the file wholesale.

## Completion Criteria

- Every reported completed task has the required verification evidence.
- Failures and blockers are visible, with no unsupported completion claims.
- `WORK_PROGRESS.md` identifies the next task or states that all requested work is verified complete.
- Existing plan and progress history are preserved unless the user explicitly authorizes a change.