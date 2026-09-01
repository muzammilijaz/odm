import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import Popup from "./Popup";

// Notification popup windows (see spawnPopup in App.tsx) load this same
// entry point in a separate Tauri window, distinguished by a `?popup=`
// query param -- everything else renders the normal main window.
const isPopup = new URLSearchParams(window.location.search).has("popup");

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>{isPopup ? <Popup /> : <App />}</React.StrictMode>,
);
