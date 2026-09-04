import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './styles/global.css'
import App from './App'
import { BusinessMonthProvider } from './contexts/BusinessMonthContext'
import { SecurityProvider } from './contexts/SecurityContext'

export function mountApp() {
  createRoot(document.getElementById('root')!).render(
    <StrictMode>
      <SecurityProvider>
        <BusinessMonthProvider>
          <App />
        </BusinessMonthProvider>
      </SecurityProvider>
    </StrictMode>,
  )
}
