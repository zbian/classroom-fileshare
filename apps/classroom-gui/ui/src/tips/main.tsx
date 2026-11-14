import React from 'react'
import ReactDOM from 'react-dom/client'
import '../tokens.css'
import '../base.css'
import './tips.css'
import TipsStack from './TipsStack'

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <TipsStack />
  </React.StrictMode>,
)
