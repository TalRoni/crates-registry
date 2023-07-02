import React, { FC, useState, useEffect } from "react";
import {
  Button,
  ListGroup,
  OverlayTrigger,
  Popover,
  Form,
} from "react-bootstrap";

import { OverlayInjectedProps } from "react-bootstrap/Overlay";

type Versions = {
  [name: string]: string[];
};

export const RustVersionsList: FC = () => {
  const [versions, setVersions] = useState<Versions>({});

  useEffect(() => {
    fetch(`api/versions`).then((response) => {
      if (response.ok) {
        response
          .json()
          .then((versionsRes) => {
            setVersions(versionsRes.versions);
          })
          .catch((err) => {
            console.log("error while parsing the json response: ", err);
            setVersions({
              "2023-03-01-nightly": [
                "x86_64-unknown-linux-gnu",
                "x86_64-windows-msvc",
              ],
              "1.67.1": ["x86_64-unknown-linux-gnu"],
              "2023-02-11-nightly": ["x86_64-unknown-linux-gnu"],
            });
          });
      }
    });
  }, []);

  return (
    <div className="ms-5">
      <ListGroup>
        {Object.entries(versions).map(([name, platforms]) => (
          <VersionItem key={name} name={name} platforms={platforms} />
        ))}
      </ListGroup>
    </div>
  );
};

type VersionItemProps = {
  name: string;
  platforms: string[];
};

const VersionItem: FC<VersionItemProps> = ({ name, platforms }) => {
  return (
    <ListGroup.Item key={name} className="d-flex justify-content-between">
      <span>{name}</span>
      <OverlayTrigger
        trigger="click"
        rootClose={true}
        placement="right"
        overlay={
          <PlatformsPopover id={`${name}-popover`} platforms={platforms} />
        }
      >
        <Button size="sm" variant="outline-secondary" className="ms-5">
          Avaliable on {platforms.length} platforms
        </Button>
      </OverlayTrigger>
    </ListGroup.Item>
  );
};

const PlatformsPopover = React.forwardRef<HTMLDivElement, OverlayInjectedProps>(
  ({ popper, children, platforms, ...props }, ref) => {
    const [filteredPlatforms, setFilteredPlatforms] =
      useState<string[]>(platforms);

    useEffect(() => {
      popper.scheduleUpdate?.();
    }, [popper]);
    return (
      <Popover body {...props} ref={ref}>
        <div className="d-flex align-items-center flex-column">
          <div className="mt-2">
            <Form.Control
              placeholder="Type to search"
              onChange={({
                target: { value },
              }: React.ChangeEvent<HTMLInputElement>) =>
                setFilteredPlatforms(
                  platforms.filter((platform: string) => platform.includes(value))
                )
              }
            />
          </div>
          <ListGroup>
            {filteredPlatforms.map((platform) => (
              <ListGroup.Item key={`${platform}`}>{platform}</ListGroup.Item>
            ))}
          </ListGroup>
        </div>
      </Popover>
    );
  }
);
