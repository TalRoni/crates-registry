import { FC } from "react";
import { RustVersionsList } from "./RustVersionsList";
import { LoadPackedFile } from "./LoadPackedFile";

export const ManageInstallations: FC = () => {
  return (
    <div>
      <h3 className="my-4 text-center">Manage Rust Installations</h3>
      <div className="d-flex flex-column align-items-start px-2">
        <div className="d-flex flex-column justify-content-start align-items-start">
          <h6>Available rust versions:</h6>
          <RustVersionsList />
        </div>
        <div className="d-flex flex-column justify-content-start align-items-start">
          <h6 className='mt-3'>Load new packed_file: </h6>
          <LoadPackedFile />
        </div>
      </div>
    </div>
  );
};
