import React, {Component} from 'react';
import {hot} from 'react-hot-loader';
import {DefaultPalette, INavLinkGroup, IStackItemStyles, IStackStyles, Nav, Stack} from '@fluentui/react';
import './App.css';
import PlaylistManager from "./PlaylistManager";

const stackStyles: IStackStyles = {
    root: {
        background: DefaultPalette.themePrimary,
        width: "100%",
        height: "100%",
    },
};

const navLinkGroups: INavLinkGroup[] = [
    {
        links: [
            {
                name: 'Home',
                key: 'home',
                url: '',
            }
        ],
    }
];

class App extends Component {
    render() {
        return (
            <div className="App">
                <Stack verticalFill={true}>
                    <Stack.Item>
                        <Stack horizontal styles={stackStyles}>
                            foo
                        </Stack>
                    </Stack.Item>
                    <Stack.Item align='stretch' grow>
                        <Stack horizontal>
                            <Stack.Item>
                                <Nav groups={navLinkGroups}/>
                            </Stack.Item>
                            <Stack.Item grow>
                                <PlaylistManager/>
                            </Stack.Item>
                        </Stack>
                    </Stack.Item>
                    <Stack.Item>
                        <Stack horizontal styles={stackStyles}>
                            baz
                        </Stack>
                    </Stack.Item>
                </Stack>
            </div>
        );
    }
}

export default hot(module)(App);
