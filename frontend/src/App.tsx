import React from 'react';
import { Routes, Route } from 'react-router-dom';
import { Layout } from './components/Layout';
import { Home } from './components/Home';
import { ManageInstallations } from './components/ManageInstallations';

function App() {
    return (
        <div className="App">
            <Routes>
                <Route path="/" element={<Layout />}>
                    <Route index element={<Home />} />
                    <Route path="/manage-installations" element={<ManageInstallations />} />
                    <Route path="*" element={<Home />} />
                </Route>
            </Routes>
        </div>
    );
}

export default App;
