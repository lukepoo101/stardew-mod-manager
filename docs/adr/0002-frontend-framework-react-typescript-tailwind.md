# ADR-0002: Frontend Framework - React 19, TypeScript, and Tailwind CSS

## Status
Accepted

## Date
2026-09-12

## Context
The user interface requires interactive state management (stepped wizard flows for game detection and SMAPI installation, drag-and-drop archive inspection, real-time log polling, mod dependency review, and modal confirmations). It must also strictly follow design accessibility standards: 16px minimum readable text, 40px minimum interactive touch targets, 44px primary action buttons, WCAG AAA/AA color contrast, and seamless Light/Dark mode transitions.

## Decision
Adopt **React 19**, **TypeScript** (strict mode enabled), and **Tailwind CSS** (via `@tailwindcss/vite` and CSS custom properties for semantic tokens) for the desktop webview layer.

## Consequences
### Positive
- Component-driven architecture with declarative UI state.
- Compile-time type safety across all domain models shared with Rust via typed IPC contracts.
- High accessibility adherence through semantic CSS design tokens (`--color-bg`, `--color-surface`, `--color-text`, `--color-primary`, `--spacing-*`, `--radius-*`) defined in `src/styles/tokens.css`.
- Tailwind v4 Vite plugin integration provides zero-runtime styling overhead and fast HMR.

### Negative
- Requires a build pipeline (Vite) and JS runtime during development.
- Tailwind class utility conventions require disciplined adherence to project semantic design tokens.

## Alternatives Considered
- **Vanilla HTML/JS**: High friction for complex state machines, modal overlays, and reactive status polling.
- **Svelte 5 / Vue 3**: Viable alternatives, but React 19 has the strongest ecosystem for testing utilities (React Testing Library) and accessible component primitives.
