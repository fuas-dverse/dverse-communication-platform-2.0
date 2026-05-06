import { defineConfig as defineViteConfig } from "vite"
import { defineConfig as defineVitestConfig, mergeConfig } from "vitest/config"
import react from "@vitejs/plugin-react"

const viteConfig = defineViteConfig({
  plugins: [react()],
})

const vitestConfig = defineVitestConfig({
  test: {
    coverage: {
      provider: 'v8',
      reporter: ['lcov', 'text'],
      reportsDirectory: './coverage',
    },
  },
})

export default mergeConfig(viteConfig, vitestConfig)