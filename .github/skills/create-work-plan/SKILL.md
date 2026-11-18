---
name: create-work-plan
description: 'Create or update a repository-root WORK_PLAN.md from a requested goal. Use for project roadmaps, implementation plans, phased task plans, and requests to document planned work.'
argument-hint: 'Describe the goal and constraints for the work plan.'
---

# Create a Work Plan

## Purpose

Turn a user-requested outcome into a repository-grounded, actionable plan and write it to `WORK_PLAN.md` at the repository root. This skill creates or updates the plan; it does not begin implementing its tasks unless asked separately.

## Workflow

1. Establish the desired outcome, scope, constraints, and any requested level of detail. Ask a concise clarifying question only when a missing decision would materially change the plan. Otherwise, state reasonable assumptions in the document.
2. Inspect the repository's existing planning and status documents first, then read only the code, tests, and configuration needed to verify current behavior and likely work areas. Reuse useful existing conventions without duplicating stale or unrelated roadmap content.
3. Separate verified current state from proposed work. Identify dependencies, ordering constraints, risks, out-of-scope items, and the evidence needed to call each phase complete. Do not invent owners, deadlines, estimates, or capabilities.
4. Organize `WORK_PLAN.md` around the requested outcome. Include the sections that fit the task:
   - Goal and scope
   - Current state and assumptions
   - Success criteria
   - Ordered phases or workstreams
   - Concrete tasks with dependencies and validation checks
   - Risks, open decisions, and out-of-scope work
   - Immediate next actions
5. Make tasks specific and verifiable. Prefer action verbs, name relevant files or commands when confirmed in the repository, and describe expected evidence rather than claiming work is already complete.
6. If `WORK_PLAN.md` already exists, read it before editing. Preserve valid progress and decisions; update only the requested plan. If it conflicts materially with the new goal or existing project commitments, ask before replacing it.
7. Write the document using the repository's existing style and Markdown conventions. Keep it focused enough to maintain, and avoid copying large sections from other documents when a short reference is sufficient.
8. Validate that the file is at the repository root, the plan has a clear outcome and actionable next step, each phase has completion evidence, and the diff has no whitespace errors. Report what was created or changed and any unresolved assumptions. Do not run planned implementation tasks unless requested.

## Quality Checks

- The plan reflects repository evidence and distinguishes facts from proposals.
- Tasks are ordered by dependencies and can be acted on without guessing the intent.
- Validation commands and acceptance criteria are realistic for this project.
- Priorities and risks are included only when supported by the request or repository context.
- The plan does not overstate test coverage, readiness, security, or capacity.