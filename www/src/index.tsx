import React, {CSSProperties} from 'react';
import ReactDOM from 'react-dom';
import App from './App';
import {initializeIcons, ThemeProvider} from '@fluentui/react';

const themeProviderStyle: CSSProperties = {
    height: "100%",
};

initializeIcons();

ReactDOM.render(
    <ThemeProvider style={themeProviderStyle}>
        <App/>
    </ThemeProvider>,
    document.getElementById('root')
);
