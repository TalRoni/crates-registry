import { FC, useState, useEffect, useCallback } from 'react';
import { Dropdown } from 'react-bootstrap';

type PlatformsDropdownProps = {
    onSelectPlatform: (platform: string) => void
};

export const PlatformsDropdown: FC<PlatformsDropdownProps> = ({ onSelectPlatform }) => {
    const [platforms, setPlatforms] = useState<string[]>();
    const [selectedPlatform, setSelectedPlatform] = useState<string>();

    const selectPlatform = useCallback((platform: string) => {
        onSelectPlatform(platform);
        setSelectedPlatform(platform);
    }, [onSelectPlatform]);

    useEffect(() => {
        fetch("api/available-platforms", { headers: [['Content-Type', 'application/json']] }).then(response => {
            if (response.ok) {
                response.json().then(platforms => {
                    setPlatforms(platforms)
                    selectPlatform(platforms[0])
                }).catch(err => {
                    console.log("Error while parsing json: ", err);
                });
            } else {
                console.log("Response error: ", response.statusText);
            }
        }).catch(err => {
            console.log("Fetch available platforms error: ", err);
        })
    }, [selectPlatform])

    return <div className='d-flex justify-content-center align-items-center'>
        <span>Choose platform:</span>
        <Dropdown>
            <Dropdown.Toggle variant="outline" id="dropdown-basic">
                {selectedPlatform}
            </Dropdown.Toggle>

            <Dropdown.Menu>
                {platforms?.map(platform => {
                    return <Dropdown.Item key={platform} onClick={() => selectPlatform(platform)}>{platform}</Dropdown.Item>
                })}
            </Dropdown.Menu>
        </Dropdown>
    </div>

}