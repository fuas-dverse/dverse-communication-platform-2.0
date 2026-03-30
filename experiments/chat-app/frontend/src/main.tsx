import { StrictMode } from "react"
import { createRoot } from "react-dom/client"
import { BrowserRouter } from "react-router-dom"
import { addCollection } from "@iconify/react"
import lucide from "@iconify-json/lucide/icons.json"
import "./index.css"
import App from "./App"

addCollection(lucide)

createRoot(document.getElementById("root")!).render(
  <StrictMode>
    <BrowserRouter>
      <App />
    </BrowserRouter>
  </StrictMode>
)
