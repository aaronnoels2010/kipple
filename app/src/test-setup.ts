// SPDX-License-Identifier: GPL-3.0-or-later
import "@testing-library/jest-dom/vitest";

import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// Testing Library only auto-unmounts when Vitest globals are enabled; they are not here.
afterEach(() => cleanup());
