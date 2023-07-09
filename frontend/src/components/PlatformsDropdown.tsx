import { FC, useState, useEffect, useCallback } from "react";
import { Dropdown } from "react-bootstrap";
import { toast } from "react-toastify";

type PlatformsDropdownProps = {
  onSelectPlatform: (platform: string) => void;
};

export const PlatformsDropdown: FC<PlatformsDropdownProps> = ({
  onSelectPlatform,
}) => {
  const [platforms, setPlatforms] = useState<string[]>();
  const [selectedPlatform, setSelectedPlatform] = useState<string>();

  const selectPlatform = useCallback(
    (platform: string) => {
      onSelectPlatform(platform);
      setSelectedPlatform(platform);
    },
    [onSelectPlatform]
  );

  const getPlatforms = useCallback(async () => {
    const response = await fetch("api/available-platforms", {
      headers: [["Content-Type", "application/json"]],
    });
    if (!response.ok) {
      throw Error(response.statusText);
    }
    const platforms = await response.json();
    setPlatforms(platforms);
    selectPlatform(platforms[0]);
  }, [selectPlatform]);

  useEffect(() => {
    toast.promise(getPlatforms(), {
      error: "Error while getting the platforms",
    });
  }, [selectPlatform]);

  return (
    <div className="d-flex justify-content-center align-items-center">
      <span>Choose platform:</span>
      <Dropdown>
        <Dropdown.Toggle variant="outline" id="dropdown-basic">
          {selectedPlatform}
        </Dropdown.Toggle>

        <Dropdown.Menu>
          {platforms?.map((platform) => {
            return (
              <Dropdown.Item
                key={platform}
                onClick={() => selectPlatform(platform)}
              >
                {platform}
              </Dropdown.Item>
            );
          })}
        </Dropdown.Menu>
      </Dropdown>
    </div>
  );
};
