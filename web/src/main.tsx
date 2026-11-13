import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { createBrowserRouter, RouterProvider } from "react-router";
import { App } from "./App";
import "@fontsource-variable/noto-sans-tc";
import "@fontsource-variable/noto-serif-tc";
import "./styles/tokens.css";
import "./styles/base.css";
import "./styles/components.css";
import "./styles/shell.css";
import "./styles/app.css";
import "./styles/phase2.css";
import "./styles/record.css";

// A data router (one splat route, the app keeps its own <Routes>) so pages can hold an
// in-app navigation while they have unsaved changes (useBlocker).
const router = createBrowserRouter([{ path: "*", element: <App /> }]);

const root = document.getElementById("root");
if (root) {
  createRoot(root).render(
    <StrictMode>
      <RouterProvider router={router} />
    </StrictMode>,
  );
}
