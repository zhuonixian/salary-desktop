import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import './styles/global.css'
import App from './App'
import { BusinessMonthProvider } from './contexts/BusinessMonthContext'
import { OperatorProvider } from './contexts/OperatorContext'
import { SecurityProvider } from './contexts/SecurityContext'

export function mountApp() {
  createRoot(document.getElementById('root')!).render(
    <StrictMode>
      <SecurityProvider>
        <OperatorProvider>
          <BusinessMonthProvider>
            <App />
          </BusinessMonthProvider>
        </OperatorProvider>
      </SecurityProvider>
    </StrictMode>,
  )
}
