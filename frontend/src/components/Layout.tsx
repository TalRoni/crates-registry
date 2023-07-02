import { FC } from 'react';
import { Link, Outlet } from 'react-router-dom';
import { Navbar, Nav, Container } from 'react-bootstrap';

export const Layout: FC = () => {
    return <div>
        <Navbar bg="light" expand="lg">
            <Container>
                <Navbar.Brand as={Link} to="/">Crates Registry</Navbar.Brand>
                <Navbar.Toggle aria-controls="basic-navbar-nav" />
                <Navbar.Collapse id="basic-navbar-nav">
                    <Nav className="me-auto">
                        <Nav.Link as={Link} to="/">Home</Nav.Link>
                        <Nav.Link as={Link} to="/manage-installations">Manage Rust installations</Nav.Link>
                    </Nav>
                </Navbar.Collapse>
            </Container>
        </Navbar>
        <Outlet />
    </div>

}