import { FC, useState } from 'react';
import Highlight from 'react-highlight';
import 'highlight.js/styles/a11y-dark.css';
import { PlatformsDropdown } from './PlatformsDropdown';

export const Home: FC = () => {
    const [platform, setPlatform] = useState<string>();

    return <div className='d-flex flex-column justify-content-start px-5'>
        <h3 className='my-4 text-center'>Welcome to Crates Registry</h3>
        <h5 className='text-start'>Configure your Rustup:</h5>
        <div className='w-100'>
            <Highlight className='text-start shell-session'>
                {
                    `echo "export RUSTUP_DIST_SERVER=http://${window.location.host}" >> ~/.bashrc
echo "export RUSTUP_UPDATE_ROOT=http://${window.location.host}/rustup" >> ~/.bashrc
source ~/.bashrc`
                }
            </Highlight>
        </div>
        <h5 className='text-start'>Configure your Cargo:</h5>
        <div className='w-100'>
            <Highlight className='text-start shell-session'>
                {
                    `mkdir -p ~/.cargo

cat <<EOT > ~/.cargo/config
[source.crates-registry]
registry = "http://${window.location.host}/git/index"
[source.crates-registry-sparse]
registry = "sparse+http://${window.location.host}/index/"

[source.crates-registry]
# To use sparse index, change "crates-registry" to "crates-registry-sparse".
replace-with = "crates-registry"
EOT
`                        }
            </Highlight>
        </div>
        <h5 className='text-start'>Run rustup-init:</h5>
        <div>
            <PlatformsDropdown onSelectPlatform={(platform) => setPlatform(platform)} />
            <Highlight className='text-start shell-session'>
                {
                    `wget http://${window.location.host}/rustup/dist/${platform ? platform : '(replace with selected platform)'}/rustup-init
chmod +x rustup-init
./rustup-init
`
                }
            </Highlight>
        </div>

    </div>
}