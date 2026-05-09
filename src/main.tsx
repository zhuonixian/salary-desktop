const diag = document.getElementById('diag')

if (diag) {
  diag.textContent += 'Step 3.1: Module entry executed\n'
}

import('./bootstrap')
  .then(({ mountApp }) => {
    if (diag) {
      diag.textContent += 'Step 3.2: React bootstrap loaded\n'
    }
    mountApp()
  })
  .catch((error) => {
    if (diag) {
      diag.textContent += 'MODULE IMPORT ERROR: ' + String(error) + '\n'
      if (error && error.stack) {
        diag.textContent += '  Stack: ' + error.stack.substring(0, 500) + '\n'
      }
    }
    throw error
  })
