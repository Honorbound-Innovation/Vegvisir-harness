/*
 *  farm9crypt.h
 *
 *  Copyright (C) 2000-2003 farm9.com, Inc.  All Rights Reserved.
 *  
 *  C interface between netcat and twofish.
 *
 *  Intended for direct replacement of system "read" and "write" calls.
 *
 *  NOTE: This file must be included within "extern C {...}" when included in C++
 *
 *  jsilva@farm9.com -- 2 December 2003, relicensed under GPL.
 */

/***************************************************************************
 *                                                                         *
 *   This program is free software; you can redistribute it and/or modify  *
 *   it under the terms of the GNU General Public License as published by  *
 *   the Free Software Foundation; either version 2 of the License, or     *
 *   (at your option) any later version.                                   *
 *                                                                         *
 *   This program is distributed in the hope that it will be useful,       *
 *   but WITHOUT ANY WARRANTY; without even the implied warranty of        *
 *   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the         *
 *   GNU General Public License for more details.                          *
 *                                                                         *
 ***************************************************************************/

void farm9crypt_init( char* inkey );
void farm9crypt_debug();
int farm9crypt_initialized();
int farm9crypt_read( int sockfd, char* buf, int size );
int farm9crypt_write( int sockfd, char* buf, int size );
